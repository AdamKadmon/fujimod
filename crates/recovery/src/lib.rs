//! SD card construction from explicit source identities and located recipes.
#![forbid(unsafe_code)]
pub mod image;
pub mod flash_free;
mod lupi;
mod lupi_loader;
pub mod rom_sd;
pub use lupi::Lupi;
mod xp4;
mod xp5;
use image::{File, Packaging, Plan, Result, require};
use serde::{Deserialize, Serialize};
pub use xp4::{ColdStart, FileApi, Setup};
pub use xp5::{
    Board as SdBoard, CodeProof as Xp5CodeProof, Flash as Xp5Flash, Lamp as Xp5Lamp,
    Operation as SdOperation, RegisterWrite as Xp5RegisterWrite, Setup as Xp5Setup,
    SourceProof as Xp5SourceProof,
};

/// Optional diagnostics for XP5 startup construction. Zero preserves normal entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StartupOptions {
    pub success_hold_seconds: u32,
    /// Hold length in loop units instead of seconds (2,048 units per second).
    /// Mutually exclusive with a nonzero `success_hold_seconds`.
    pub success_hold_units: Option<u64>,
}

impl StartupOptions {
    /// The hold in loop units of 1,000,000 four-NOP iterations.
    pub fn hold_units(&self) -> Result<u64> {
        require(
            self.success_hold_units.is_none() || self.success_hold_seconds == 0,
            "Success hold takes seconds or units, not both",
        )?;
        Ok(self
            .success_hold_units
            .unwrap_or(u64::from(self.success_hold_seconds) * 2_048))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Generation {
    Exr,
    #[serde(rename = "exrpro")]
    ExrPro,
    Exr2,
    #[serde(rename = "xppro")]
    XpPro,
    Xp4,
    Xp5,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub address: usize,
    pub size: usize,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum VersionConfiguration {
    Reader {
        address: usize,
    },
    Memory {
        pointer: usize,
        selector_offset: usize,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionInput {
    pub address: usize,
    pub value: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionContract {
    pub public_version: [u8; 2],
    pub version_getter: Proof,
    pub configuration: VersionConfiguration,
    pub getter_arguments: u8,
    pub comparison: Proof,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comparison_inputs: Vec<VersionInput>,
    pub version_producer: Proof,
    pub producer_stop: usize,
    pub version_context_pointer: usize,
    pub version_context: usize,
    pub current_major_setter: Proof,
    pub current_minor_setter: Proof,
    pub target_major: usize,
    pub target_minor: usize,
    pub current_major: usize,
    pub current_minor: usize,
}
impl VersionContract {
    fn verify(&self, ram: &[u8]) -> Result<()> {
        for proof in [
            &self.version_getter,
            &self.comparison,
            &self.version_producer,
            &self.current_major_setter,
            &self.current_minor_setter,
        ] {
            image::hash(
                image::extent(ram, proof.address, proof.size)?,
                &proof.sha256,
            )?;
        }
        require(
            self.current_major == self.target_major + 8
                && self.current_minor == self.target_minor + 8
                && self.target_minor == self.target_major + 4,
            "Updater version pair layout differs",
        )?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub firmware_id: String,
    pub source_size: usize,
    pub updater: Option<VersionContract>,
    #[serde(default)]
    pub flash_free: Option<flash_free::FlashFreeProof>,
    pub source_sha256: String,
    pub update_filename: String,
    pub generation: Generation,
    pub plan: Option<Plan>,
    pub packaging: Option<Packaging>,
    pub lupi: Option<Lupi>,
    pub setup: Option<Setup>,
    pub xp5: Option<Xp5Setup>,
    pub cold_start: Option<ColdStart>,
    pub file_api: Option<FileApi>,
    pub sd_boot: Option<serde_json::Value>,
    pub firmware_update: Option<serde_json::Value>,
    pub missing: Vec<String>,
}
impl Recipe {
    pub fn missing_facts(&self) -> Vec<String> {
        let mut missing = self.missing.clone();
        if self.updater.is_none()
            && !missing
                .iter()
                .any(|s| s == "version-independent stock updater acceptance")
        {
            missing.push("version-independent stock updater acceptance".into());
        }
        if self
            .flash_free
            .as_ref()
            .is_none_or(|proof| proof.verify(self).is_err())
            && !missing.iter().any(|s| s == flash_free::MISSING)
        {
            missing.push(flash_free::MISSING.into());
        }
        missing
    }
    pub fn bundled(firmware_id: &str) -> Result<Option<Self>> {
        let recipes: Vec<Self> = serde_json::from_str(include_str!(concat!(
            env!("OUT_DIR"),
            "/recipes.generated.json"
        )))
        .map_err(|e| image::Error::Invalid(e.to_string()))?;
        Ok(recipes.into_iter().find(|r| r.firmware_id == firmware_id))
    }
    pub fn cold_worker(&self, source: &[u8]) -> Result<serde_json::Value> {
        require(
            source.len() == self.source_size,
            "Update size differs from recipe",
        )?;
        image::hash(source, &self.source_sha256)?;
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| image::Error::Invalid("Missing RAM plan".into()))?;
        let cold = self
            .cold_start
            .as_ref()
            .ok_or_else(|| image::Error::Invalid("Missing cold startup".into()))?;
        let loader = plan
            .regions
            .first()
            .ok_or_else(|| image::Error::Invalid("Missing loader".into()))?
            .source
            .read(&fujimod_firmware::invert(source))?;
        serde_json::to_value(xp4::worker(&loader, cold.mode_flag)?)
            .map_err(|e| image::Error::Invalid(e.to_string()))
    }
    pub fn cold_plan(&self, source: &[u8]) -> Result<Plan> {
        require(
            source.len() == self.source_size,
            "Update size differs from recipe",
        )?;
        image::hash(source, &self.source_sha256)?;
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| image::Error::Invalid("Missing RAM plan".into()))?;
        let cold = self
            .cold_start
            .as_ref()
            .ok_or_else(|| image::Error::Invalid("Missing cold startup".into()))?;
        xp4::cold_plan(&fujimod_firmware::invert(source), plan, cold)
    }
    /// Explicit SRAM placement and file ABI for isolated loader execution.
    pub fn compile_ffinit(
        &self,
        source: &[u8],
        payload: &[u8],
        base: u32,
        api: &FileApi,
        workspace: u32,
    ) -> Result<(Vec<u8>, serde_json::Value)> {
        require(
            source.len() == self.source_size,
            "Update size differs from recipe",
        )?;
        image::hash(source, &self.source_sha256)?;
        let setup = self
            .setup
            .as_ref()
            .ok_or_else(|| image::Error::Invalid("Missing DDR setup".into()))?;
        let bytes = xp4::ffinit(
            &fujimod_firmware::invert(source),
            setup,
            api,
            payload,
            base,
            Some(workspace),
        )?;
        let mut layout = xp4::layout(setup, api, base);
        layout["workspace"] = workspace.into();
        layout["file_api"] = serde_json::json!(api);
        layout["payload_size"] = payload.len().into();
        layout["payload_crc32"] = xp4::crc32(payload).into();
        layout["bytes"] = bytes.len().into();
        layout["sha256"] = fujimod_firmware::sha256(&bytes).into();
        Ok((bytes, layout))
    }
    /// Prepare XP5 flash and resume the update's original internal startup.
    pub fn build_stage_one(&self, source: &[u8]) -> Result<Vec<File>> {
        self.build_stage_one_with_options(source, StartupOptions::default())
    }
    pub fn build_stage_one_with_options(
        &self,
        source: &[u8],
        options: StartupOptions,
    ) -> Result<Vec<File>> {
        require(
            matches!(self.generation, Generation::Xp5),
            "Stage one requires XP5",
        )?;
        require(
            source.len() == self.source_size,
            "Update size differs from recipe",
        )?;
        image::hash(source, &self.source_sha256)?;
        let setup = self.xp5.as_ref().ok_or_else(|| {
            image::Error::Invalid(format!(
                "Missing XP5 flash/setup bindings: {}",
                self.missing.join(", ")
            ))
        })?;
        xp5::package(
            &fujimod_firmware::invert(source),
            setup,
            None,
            options.hold_units()?,
        )
    }
    /// Boot files retain the stock updater and storage drivers. DAT is copied
    /// verbatim to its required root filename, including signed containers.
    pub fn build(&self, source: &[u8]) -> Result<Vec<File>> {
        let missing = self.missing_facts();
        require(
            missing.is_empty(),
            &format!("Missing recovery facts: {}", missing.join(", ")),
        )?;
        require(
            self.updater.is_some(),
            "Missing version-independent stock updater acceptance",
        )?;
        let files = self.build_startup(source)?;
        self.flash_free.as_ref().unwrap().verify_files(&files)?;
        Ok(files)
    }
    /// Construct established startup files independently of updater readiness.
    pub fn build_startup(&self, source: &[u8]) -> Result<Vec<File>> {
        self.build_startup_with_options(source, StartupOptions::default())
    }
    pub fn build_startup_with_options(
        &self,
        source: &[u8],
        options: StartupOptions,
    ) -> Result<Vec<File>> {
        require(
            options.hold_units()? == 0 || matches!(self.generation, Generation::Xp5),
            "Success hold requires XP5",
        )?;
        require(
            source.len() == self.source_size,
            "Update size differs from recipe",
        )?;
        image::hash(source, &self.source_sha256)?;
        let inverted = fujimod_firmware::invert(source);
        let plan = self.plan.as_ref().ok_or_else(|| {
            image::Error::Invalid(format!(
                "Missing RAM image plan: {}",
                self.missing.join(", ")
            ))
        })?;
        let mut files =
            match self.generation {
                Generation::Exr | Generation::ExrPro => {
                    let header = self
                        .packaging
                        .as_ref()
                        .ok_or_else(|| image::Error::Invalid("Missing Lupi header".into()))?;
                    let api = self.lupi.as_ref().ok_or_else(|| {
                        image::Error::Invalid("Missing Lupi filesystem ABI".into())
                    })?;
                    lupi::package(
                        api,
                        header,
                        &inverted,
                        plan,
                        image::transform(&inverted, plan)?,
                    )?
                }
                Generation::Exr2 | Generation::XpPro => {
                    let packaging = self.packaging.as_ref().ok_or_else(|| {
                        image::Error::Invalid("Missing SD header packaging".into())
                    })?;
                    packaging.package(&inverted, plan, image::transform(&inverted, plan)?)?
                }
                Generation::Xp4 => {
                    let cold = self
                        .cold_start
                        .as_ref()
                        .ok_or_else(|| image::Error::Invalid("Missing XP4 cold startup".into()))?;
                    let setup = self
                        .setup
                        .as_ref()
                        .ok_or_else(|| image::Error::Invalid("Missing XP4 DDR setup".into()))?;
                    let api = self.file_api.as_ref().ok_or_else(|| {
                        image::Error::Invalid("Missing Topaz filesystem ABI".into())
                    })?;
                    let plan = xp4::cold_plan(&inverted, plan, cold)?;
                    let ram = image::transform(&inverted, &plan)?;
                    let init = xp4::ffinit(&inverted, setup, api, &ram, api.load_address, None)?;
                    vec![
                        File::new("ffinit.bin", init)?,
                        File::new("ffboot.bin", ram)?,
                    ]
                }
                Generation::Xp5 => {
                    let setup = self.xp5.as_ref().ok_or_else(|| {
                        image::Error::Invalid(
                            "Missing XP5 post-setup SD loader and handoff facts".into(),
                        )
                    })?;
                    xp5::package(
                        &inverted,
                        setup,
                        Some(image::transform(&inverted, plan)?),
                        options.hold_units()?,
                    )?
                }
            };
        require(
            !files
                .iter()
                .any(|f| f.name.eq_ignore_ascii_case(&self.update_filename)),
            "DAT filename collides with boot files",
        )?;
        if let Some(contract) = &self.updater {
            let ram_filename = if matches!(self.generation, Generation::Exr | Generation::ExrPro) {
                "ffimage.bin"
            } else {
                "ffboot.bin"
            };
            let ram = files
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(ram_filename))
                .ok_or_else(|| {
                    image::Error::Invalid("Missing RAM image for updater proof".into())
                })?;
            contract.verify(&ram.bytes)?;
        }
        files.push(File::new(&self.update_filename, source.to_vec())?);
        Ok(files)
    }
}
#[derive(Debug, Serialize)]
pub struct Availability {
    pub firmware_id: String,
    pub generation: Option<Generation>,
    pub can_build: bool,
    pub missing: Vec<String>,
    pub output_filenames: Vec<String>,
    pub sd_boot: Option<serde_json::Value>,
    pub firmware_update: Option<serde_json::Value>,
}
pub fn availability(firmware_id: &str) -> Result<Availability> {
    let id = firmware_id;
    let recipe = Recipe::bundled(id)?;
    let sd_boot = recipe.as_ref().and_then(|r| r.sd_boot.clone());
    let firmware_update = recipe.as_ref().and_then(|r| r.firmware_update.clone());
    let missing = recipe
        .as_ref()
        .map(|r| r.missing_facts())
        .unwrap_or_else(|| vec!["recovery recipe".into()]);
    let mut output_filenames = Vec::new();
    if let Some(recipe) = &recipe {
        match recipe.generation {
            Generation::Exr | Generation::ExrPro | Generation::Exr2 | Generation::XpPro => {
                if let Some(packaging) = &recipe.packaging {
                    output_filenames.extend([
                        packaging.header_filename.clone(),
                        packaging.image_filename.clone(),
                    ]);
                }
                if matches!(recipe.generation, Generation::Exr | Generation::ExrPro) {
                    output_filenames.push("ffimage.bin".into());
                }
            }
            Generation::Xp4 => output_filenames.extend(["ffinit.bin".into(), "ffboot.bin".into()]),
            Generation::Xp5 => {
                output_filenames.extend(["marble_init.bin".into(), "ffboot.bin".into()])
            }
        }
        output_filenames.push(recipe.update_filename.clone());
    }
    Ok(Availability {
        firmware_id: id.into(),
        generation: recipe.map(|r| r.generation),
        can_build: missing.is_empty(),
        missing,
        output_filenames,
        sd_boot,
        firmware_update,
    })
}

#[cfg(test)]
mod tests;
