//! Offline construction and catalog inspection. No device access.
use fujimod_recovery::{Recipe, StartupOptions, availability};
use std::{env, fs, path::PathBuf};
fn construction_args(
    args: &[String],
    allow_hold: bool,
) -> Result<(&str, &str, &str, StartupOptions), Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut hold = None;
    let mut units = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--success-hold-seconds" {
            if !allow_hold {
                return Err("--success-hold-seconds requires startup or stage-one".into());
            }
            if hold.is_some() {
                return Err("--success-hold-seconds was specified twice".into());
            }
            let value = args.next().ok_or("Missing --success-hold-seconds value")?;
            hold =
                Some(value.parse::<u32>().map_err(
                    |_| "--success-hold-seconds requires a non-negative 32-bit integer",
                )?);
        } else if arg == "--success-hold-units" {
            if !allow_hold {
                return Err("--success-hold-units requires startup or stage-one".into());
            }
            if units.is_some() {
                return Err("--success-hold-units was specified twice".into());
            }
            let value = args.next().ok_or("Missing --success-hold-units value")?;
            units = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| "--success-hold-units requires a non-negative 64-bit integer")?,
            );
        } else if arg.starts_with("--") {
            return Err(format!("Unknown construction option: {arg}").into());
        } else {
            positional.push(arg.as_str());
        }
    }
    if positional.len() != 3 {
        return Err("Expected FIRMWARE DAT NEW_DIRECTORY".into());
    }
    if hold.is_some() && units.is_some() {
        return Err(
            "--success-hold-seconds and --success-hold-units are mutually exclusive".into(),
        );
    }
    Ok((
        positional[0],
        positional[1],
        positional[2],
        StartupOptions {
            success_hold_seconds: hold.unwrap_or(0),
            success_hold_units: units,
        },
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => {
            let recipes: Vec<Recipe> = serde_json::from_str(include_str!(concat!(
                env!("OUT_DIR"),
                "/recipes.generated.json"
            )))?;
            let rows = recipes
                .iter()
                .map(|r| availability(&r.firmware_id))
                .collect::<Result<Vec<_>, _>>()?;
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Some(command @ ("build" | "startup" | "stage-one")) => {
            let (firmware, dat, output, options) =
                construction_args(&args[1..], command != "build")?;
            let recipe = Recipe::bundled(firmware)?.ok_or("Missing recovery recipe")?;
            let source = fs::read(dat)?;
            let files = if command == "build" {
                recipe.build(&source)?
            } else if command == "stage-one" {
                recipe.build_stage_one_with_options(&source, options)?
            } else {
                recipe.build_startup_with_options(&source, options)?
            };
            let output = PathBuf::from(output);
            fs::create_dir(&output)?;
            let mut rows = Vec::new();
            for file in files {
                fs::write(output.join(&file.name), &file.bytes)?;
                rows.push(serde_json::json!({"name":file.name,"size":file.bytes.len(),"sha256":fujimod_firmware::sha256(&file.bytes)}));
            }
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        _ => {
            return Err(
                "Usage: fujimod-recovery list | build|startup|stage-one FIRMWARE DAT NEW_DIRECTORY [--success-hold-seconds N | --success-hold-units N]"
                    .into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::construction_args;
    use fujimod_recovery::StartupOptions;
    fn seconds(n: u32) -> StartupOptions {
        StartupOptions {
            success_hold_seconds: n,
            success_hold_units: None,
        }
    }
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn success_hold_defaults_to_zero_and_accepts_either_position() {
        assert_eq!(
            construction_args(&args(&["camera", "update", "output"]), true).unwrap(),
            ("camera", "update", "output", seconds(0))
        );
        for values in [
            args(&["camera", "update", "output", "--success-hold-seconds", "3"]),
            args(&["--success-hold-seconds", "3", "camera", "update", "output"]),
        ] {
            assert_eq!(
                construction_args(&values, true).unwrap(),
                ("camera", "update", "output", seconds(3))
            );
        }
    }
    #[test]
    fn invalid_or_unsupported_holds_refuse_before_reading_files() {
        for value in ["-1", "1.5", "text", "4294967296"] {
            assert!(
                construction_args(
                    &args(&[
                        "camera",
                        "update",
                        "output",
                        "--success-hold-seconds",
                        value
                    ]),
                    true
                )
                .is_err()
            );
        }
        assert!(
            construction_args(
                &args(&["camera", "update", "output", "--success-hold-seconds"]),
                true
            )
            .is_err()
        );
        assert!(
            construction_args(
                &args(&[
                    "camera",
                    "update",
                    "output",
                    "--success-hold-seconds",
                    "3",
                    "--success-hold-seconds",
                    "0"
                ]),
                true
            )
            .is_err()
        );
        assert!(
            construction_args(
                &args(&["camera", "update", "output", "--success-hold-seconds", "3"]),
                false
            )
            .is_err()
        );
    }
    #[test]
    fn success_hold_units_are_explicit_and_exclusive_with_seconds() {
        let units = |n: u64| StartupOptions {
            success_hold_seconds: 0,
            success_hold_units: Some(n),
        };
        for values in [
            args(&["camera", "update", "output", "--success-hold-units", "32"]),
            args(&["--success-hold-units", "32", "camera", "update", "output"]),
        ] {
            assert_eq!(
                construction_args(&values, true).unwrap(),
                ("camera", "update", "output", units(32))
            );
        }
        assert_eq!(units(32).hold_units().unwrap(), 32);
        assert_eq!(seconds(3).hold_units().unwrap(), 6_144);
        assert_eq!(StartupOptions::default().hold_units().unwrap(), 0);
        let refused = [
            // both options, either order
            vec!["--success-hold-seconds", "3", "--success-hold-units", "32"],
            vec!["--success-hold-units", "32", "--success-hold-seconds", "3"],
            // repeated, malformed, missing, unsupported for `build`
            vec!["--success-hold-units", "1", "--success-hold-units", "2"],
            vec!["--success-hold-units", "-1"],
            vec!["--success-hold-units", "1.5"],
            vec!["--success-hold-units", "18446744073709551616"],
            vec!["--success-hold-units"],
        ];
        for extra in refused {
            let mut values = args(&["camera", "update", "output"]);
            values.extend(extra.iter().map(|s| s.to_string()));
            assert!(construction_args(&values, true).is_err(), "{extra:?}");
        }
        assert!(
            construction_args(
                &args(&["camera", "update", "output", "--success-hold-units", "32"]),
                false
            )
            .is_err()
        );
        // The library refuses both settings at once.
        let both = StartupOptions {
            success_hold_seconds: 3,
            success_hold_units: Some(32),
        };
        assert!(both.hold_units().is_err());
    }
}
