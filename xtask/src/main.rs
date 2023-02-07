mod versioning;
use versioning::*;
mod utils;
use utils::*;
mod builder;
use builder::*;

use std::env;

/// target triple for precursor builds
const TARGET_TRIPLE: &str = "riscv32imac-unknown-xous-elf";

// because I have nowhere else to note this. The commit that contains the rkyv-enum derive
// refactor to work around warnings thrown by Rust 1.64.0 is: f815ed85b58b671178fbf53b4cea34186fc406eb
// We could undo this if it turns out to be a compiler regression.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = Builder::new();
    // encodes a timestamp into the build, unless '--no-timestamp' is passed
    let do_version = env::args().filter(|x| x == "--no-timestamp").count() == 0;
    generate_version(do_version);
    if do_version {
        builder.add_feature("timestamp");
    };

    // packages located on crates.io. For testing non-local build configs that are less
    // concerned about software supply chain and more focused on developer convenience.
    let base_pkgs_remote = [
        "xous-log",
        "xous-names",
        "xous-ticktimer",
    ].to_vec();
    // packages in the user image - most of the services at this layer have cross-dependencies
    let user_pkgs = [
        &base_pkgs_remote[..],
        &[
            "console",
        ]
    ].concat();

    // ---- extract position independent args ----
    let lkey = get_flag("--lkey")?;
    if lkey.len() != 0 {
        builder.loader_key_file(lkey[0].to_string());
    }
    let kkey = get_flag("--kkey")?;
    if kkey.len() != 0 {
        builder.kernel_key_file(kkey[0].to_string());
    }

    let extra_services = get_flag("--service")?;
    builder.add_services(&extra_services);
    // extract features, and especially track language features
    let features = get_flag("--feature")?;
    let mut language_set = false;
    for feature in features {
        builder.add_feature(&feature);
        if feature.starts_with("xous/lang-") {
            track_language_changes(&feature)?;
            language_set = true;
        }
    }
    let kernel_features = get_flag("--kernel-feature")?;
    for feature in kernel_features {
        builder.add_kernel_feature(&feature);
    }
    if !language_set { // the default language is english
        track_language_changes("en")?;
    }

    // ---- now process the verb plus position dependent arguments ----
    let mut args = env::args();
    let task = args.nth(1);
    match task.as_deref() {
        Some("install-toolkit") | Some("install-toolchain") => {
            let arg = env::args().nth(2);
            ensure_compiler(
                &Some(TARGET_TRIPLE),
                true,
                arg.map(|x| x == "--force").unwrap_or(false),
            )?
        }
        // ----- renode configs --------
        Some("renode-image") => {
            builder.target_renode()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_services(&get_cratespecs());
        }
        Some("renode-remote") => {
            builder.target_renode()
                   .add_services(&base_pkgs_remote.into_iter().map(String::from).collect())
                   .remove_feature("timestamp"); // crates.io package can't have a timestamp
        }

        // ------- hosted mode configs -------
        Some("run") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .add_feature("pddbtest")
                   .add_feature("ditherpunk")
                   .add_feature("tracking-alloc")
                   .add_feature("tls")
                   // .add_feature("test-rekey")
                   .add_services(&get_cratespecs());
        }
        Some("hosted-ci") => {
            builder.target_hosted()
                   .add_services(&user_pkgs.into_iter().map(String::from).collect())
                   .hosted_build_only()
                   .add_services(&get_cratespecs());
        }

        // ------ Precursor hardware image configs ------
        Some("hw-image") => {
            builder.target_cramium()
                   .add_services(&get_cratespecs());
            for service in user_pkgs {
                builder.add_service(service, true);
            }
        }

        // ---- other single-purpose commands ----
        Some("generate-locales") => generate_locales()?,
        _ => print_help(),
    }
    builder.build()?;

    Ok(())
}

fn print_help() {
    eprintln!(
"cargo xtask [verb] [cratespecs ..]
    [--feature [feature name]]
    [--lkey [loader key]] [--kkey [kernel key]]
    [--app [cratespec]]
    [--service [cratespec]]
    [--no-timestamp]
    [--no-verify]

[cratespecs] is a list of 0 or more items of the following syntax:
   [name]                crate 'name' to be built from local source
   [name@version]        crate 'name' to be fetched from crates.io at the specified version
   [name#URL]            pre-built binary crate of 'name' downloaded from a server at 'URL'
   [path-to-binary]      file path to a prebuilt binary image on local machine.
                         Files in '.' must be specified as './file' to avoid confusion with local source

The [cratespecs] list is treated as apps or services based on the context of [verb]. Additional crates can
be merged in with explicit app/service treatment with the following flags:
 [--app] [cratespec]     [cratespec] is treated as an additional app
 [--service] [cratespec] [cratespec] is treated as an additional service

[--lkey] and [--kkey]    Paths to alternate private key files for loader and kernel key signing (defaults to developer key)
[--no-timestamp]         Do not include a timestamp in the build. By default, `ticktimer` is rebuilt on every run to encode a timestamp.
[--no-verify]            Do not verify that local sources match crates.io downloaded sources

- An 'app' must be enumerated in apps/manifest.json.
   A pre-processor configures the launch menu based on the list of specified apps.
- A 'service' is merged into the device image without any pre-processing.

[verb] options:
Hardware images:
 app-image               Precursor user image. [cratespecs] are apps

Hosted emulation:
 run                     Run user image in hosted mode with release flags. [cratespecs] are apps
 hosted-ci               Hosted mode CI image

Renode emulation:
 renode-image            Renode user image. Unspecified [cratespecs] are apps

Other commands:
 generate-locales        (re)generate the locales include for the language selected in xous-rs/src/locale.rs
 install-toolkit         installs Xous toolkit with no prompt, useful in CI. Specify `--force` to remove existing toolchains

Note: By default, the `ticktimer` will get rebuilt every time. You can skip this by appending `--no-timestamp` to the command.
"
    )
}

type DynError = Box<dyn std::error::Error>;

enum MemorySpec {
    SvdFile(String),
}

/// [cratespecs] are positional arguments, and is a list of 0 to N tokens that immediately
/// follow [verb]
fn get_cratespecs() -> Vec<String> {
    let mut cratespecs = Vec::<String>::new();
    let mut args = env::args();
    args.nth(1); // skip the verb
    for arg in args {
        if arg.starts_with('-') {
            // stop processing the list as soon as first named argument is found
            break;
        }
        cratespecs.push(arg)
    }
    cratespecs
}

fn get_flag(flag: &str) -> Result<Vec<String>, DynError> {
    let mut list = Vec::<String>::new();
    let args = env::args();
    let mut flag_found = false;
    for arg in args {
        if arg == flag {
            flag_found = true;
            continue
        }
        if flag_found {
            if arg.starts_with('-') {
                eprintln!("Malformed arguments. Expected argument after flag {}, but found {}", flag, arg);
                return Err("Bad arguments".into());
            }
            list.push(arg);
            flag_found = false;
            continue
        }
    }
    Ok(list)
}
