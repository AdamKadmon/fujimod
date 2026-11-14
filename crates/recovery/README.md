# SD recovery cards

`fujimod-recovery` constructs SD startup files from an official update supplied
by the caller. It checks the complete DAT identity, decoded source hashes,
initialized mappings, BSS, expected old words, header policy, and file names.
The official DAT is included unchanged at its required card-root name.

The processor selects the packaging mechanism. EXR II uses the stock NAND
header fragments with SD power hold and a preloaded RAM image. XP4 uses raw
FFINIT with the update's DDR setup and relocated tables, the Topaz filesystem
ABI, and a cold preloaded RAM image. Reset mode remains cold; three checked
comparisons select the preloaded table, images, and worker bypass.

EXR and EXR Pro use Lupi's 512-byte `ffsys.bin` header and a small
`ffboot.bin`. That guest copies itself to SRAM and uses the ROM's u16 read
ABI to load `ffimage.bin` in chunks, checking file length, actual counts and
CRC32 before entering the stock RAM image. The extra file avoids the ROM's
16-bit truncation of the initial boot-file length. Updater code proofs are
checked against `ffimage.bin`.

X-Processor Pro uses Agate's 2 KiB DDR configuration as `ffsys.bin` and the
complete RAM image as `ffboot.bin`. The locator follows the update's single
or initial/trained DDR-copy helper to choose the header. Three checked
comparisons select the stock preloaded table, header and image paths while
retaining cold reset mode. Both families use rooted section tables and checked
source mappings; unrecognized table or update-device layouts refuse.

XP5 emits raw `marble_init.bin` with the update's setup at byte zero. Its
entry guest preserves registers and stack, initializes serial NOR access,
scans read timing, copies the complete source header into the boot-service
area, applies header operations, and restores the setup entry. Device bases,
flash pins, SD routing/power/clock operations, and status-lamp fields are
catalog inputs with checked source spans. The guest makes no ROM calls.

Startup and recovery use this same entry guest, then load FAT32 `FFBOOT.BIN`
after stock DDR setup and check its exact length and CRC32 before publishing
the setup gate. The gate applies to cores executing that bootstrap. Stock
setup and later startup retain internal-flash dependencies. Missing device or
setup facts produce specific construction refusals. `Recipe::build_stage_one`
uses the same guest and preserves the original internal-flash continuation;
it produces only `marble_init.bin`.

`availability(firmware_id)` returns `can_build`, the exact missing facts, output
file names, and the catalog's SD-boot and firmware-update button combinations.
A recovery build requires complete facts, including version-independent
acceptance by the stock updater. Located stock getters and producers supply the
SD-running firmware's own public version; the stock comparison accepts equality,
so the paired official DAT needs no version change. The recipe pins those
code spans and verifies them against the constructed RAM image. `Recipe::build_startup` constructs boot files
separately for execution and transfer tests.

JavaScript uses `recovery.availability(firmwareId)` and
`recovery.build(firmwareId, datBytes)`. Call `card.takeFiles()` to obtain file
objects with `name`, `size`, and `bytes: Uint8Array`. Free the file objects and
card after copying their contents to downloads or an archive. Construction has
no filesystem, USB, camera, or network access.

The native CLI exposes `list`, `build FIRMWARE DAT NEW_DIRECTORY`, and
`startup FIRMWARE DAT NEW_DIRECTORY`, and `stage-one FIRMWARE DAT NEW_DIRECTORY`.

`startup` and `stage-one` accept `--success-hold-seconds N` (default `0`).
After sampling and header operations succeed, a nonzero value holds steady
green using a counted loop before continuing. Each second requests 2,048 units
of 1,000,000 four-NOP iterations, allowing up to two billion iterations per
second; the startup clock can make the hold longer. This early hold calls no
stock code and requires no initialized timer. Zero skips the hold. The library
exposes the same setting through `StartupOptions` and the corresponding `_with_options` methods.

`--success-hold-units N` sets the hold directly in those loop units instead of
seconds (so `--success-hold-seconds S` equals `--success-hold-units` `S * 2048`) and
is mutually exclusive with `--success-hold-seconds`. The units are only a count of
loop passes, not a duration: the processor's speed at this point decides how long
one unit takes. Only the 64-bit hold word of the guest configuration changes.

The guest loader sources are in `guest/`. Its checked-in generated Rust
template contains the compiled loader and relocations, with no update or ROM
data. To regenerate it, set `FFINIT_CC` to GNU GCC 4.6.4 and `FFINIT_LD` to GNU
ld, then run:

```sh
cargo run -p fujimod-recovery --example generate_ffinit
cargo run -p fujimod-recovery --example generate_ffinit -- --check
```

The generated template preserves the loader's declaration and link order.
Runtime relocation inserts the supplied update's setup tables and updates all
absolute references, transfer size, CRC32, workspace, and filesystem calls.

The XP5 template is regenerated with GNU AArch64 GCC 9.2.1 and GNU ld:

```sh
XP5_CC=/path/to/aarch64-none-elf-gcc XP5_LD=/path/to/aarch64-none-elf-ld \
  cargo run -p fujimod-recovery --example generate_xp5
```

The XP5 generator checks the pre-setup call graph and GCC stack reports,
rejects unsupported relocations, and requires GCC 9.2.1. Append `-- --check`
to compare it with the checked-in template. Both template
generators use empty camera-data placeholders; card construction needs only
the official DAT and the compiled catalog recipes.

Generate and check the Lupi guest with GNU GCC 4.6.4 and GNU ld:

```sh
LUPI_CC=/path/to/arm-gcc LUPI_LD=/path/to/arm-ld \
  cargo run -p fujimod-recovery --example generate_lupi -- --check
```

`cargo run -p fujimod-recovery --example locate_rom_sd -- exr UPDATE.DAT`
emits freshly located startup plans and header facts from the supplied update.
Use `exrpro` or `xppro` for the other processor families.
