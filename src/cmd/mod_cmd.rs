use crate::errors::*;

use crate::args;
use crate::args::Install;
use crate::api::Client;
use crate::config::Config;
use colored::Colorize;
use crate::engine::Module;
use crate::registry;
use crate::shell::Readline;
use crate::update::AutoUpdater;
use std::fmt::Write;
use structopt::StructOpt;
use structopt::clap::AppSettings;
use crate::term;
use crate::worker;


#[derive(Debug, StructOpt)]
#[structopt(author = "",
            raw(global_settings = "&[AppSettings::ColoredHelp]"))]
pub struct Args {
    #[structopt(subcommand)]
    pub subcommand: SubCommand,
}

#[derive(Debug, StructOpt)]
pub enum SubCommand {
    /// List installed modules
    #[structopt(author="", name="list")]
    List(List),
    /// Install module from registry
    #[structopt(author="", name="install")]
    Install(args::Install),
    /// Search modules in registry
    #[structopt(author="", name="search")]
    Search(args::Search),
    /// Reload modules
    #[structopt(author="", name="reload")]
    Reload(Reload),
    /// Update modules
    #[structopt(author="", name="update")]
    Update(Update),
}

#[derive(Debug, StructOpt)]
pub struct List {
    /// Only show modules with a specific input source
    #[structopt(long="source")]
    pub source: Option<String>,
    /// List outdated modules
    #[structopt(long="outdated")]
    pub outdated: bool,
}

#[derive(Debug, StructOpt)]
pub struct Reload {
}

#[derive(Debug, StructOpt)]
pub struct Update {
}

fn update(client: &Client, config: &Config, module: &Module) -> Result<()> {
    let name = module.canonical();
    let installed = module.version();

    let label = format!("Searching for updates {}", name);
    let infos = worker::spawn_fn(&label, || {
        client.query_module(&module.id())
    }, true)?;
    debug!("Latest version: {:?}", infos);

    let latest = infos.latest.ok_or_else(|| format_err!("Module doesn't have any released versions"))?;

    if installed != latest {
        let label = format!("Updating {}: {:?} -> {:?}", &name, installed, latest);
        worker::spawn_fn(&label, || {
            registry::run_install(&Install {
                module: module.id(),
                version: None,
            }, &config)
        }, true)?;

        term::success(&format!("Updated {}: {:?} -> {:?}", &name, installed, latest));
    }

    Ok(())
}

pub fn run(rl: &mut Readline, args: &[String]) -> Result<()> {
    let args = Args::from_iter_safe(args)?;
    let config = rl.config().clone();

    match args.subcommand {
        SubCommand::List(list) => {
            let autoupdate = AutoUpdater::load()?;

            for module in rl.engine().list() {
                if let Some(source) = &list.source {
                    if !module.source_equals(&source) {
                        continue;
                    }
                }

                let canonical = module.canonical();

                let mut out = String::new();
                write!(&mut out, "{} ({})", canonical.green(),
                                            module.version().yellow())?;
                if autoupdate.is_outdated(&canonical) {
                    write!(&mut out, " {}", "[outdated]".red())?;
                } else if list.outdated {
                    continue;
                }
                println!("{}", out);
                println!("\t{}", module.description());
            }
        },
        SubCommand::Install(install) => {
            registry::run_install(&install, &config)?;
            // trigger reload
            run(rl, &[String::from("mod"), String::from("reload")])?;
        },
        SubCommand::Search(search) => registry::run_search(rl.engine(), &search, &config)?,
        SubCommand::Reload(_) => {
            let current = rl.take_module()
                            .map(|m| m.canonical());

            rl.engine_mut().reload_modules()?;
            rl.reload_module_cache();

            if let Some(module) = current {
                if let Ok(module) = rl.engine().get(&module).map(|x| x.to_owned()) {
                    rl.set_module(module);
                }
            }
        },
        SubCommand::Update(_) => {
            let client = Client::new(&config)?;

            let mut autoupdate = AutoUpdater::load()?;

            for module in rl.engine().list() {
                let canonical = module.canonical();

                if module.is_private() {
                    debug!("{} is a private module, skipping", canonical);
                    continue;
                }

                if let Err(err) = update(&client, &config, &module) {
                    term::error(&format!("Failed to update {}: {}", canonical, err));
                } else {
                    autoupdate.updated(&canonical);
                }
            }

            autoupdate.save()?;

            // trigger reload
            run(rl, &[String::from("mod"), String::from("reload")])?;
        },
    }

    Ok(())
}
