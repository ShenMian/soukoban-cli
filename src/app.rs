//! Application setup, dispatch, and top level error handling.

use std::{
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context as _;
use ctrlc::Error as CtrlcError;

use crate::{
    cli::{Cli, Command, GlobalArgs},
    commands::{search, solve},
    error::{self, EXIT_INTERRUPT},
    ui::{
        Ui,
        theme::{Glyphs, Theme},
    },
};

/// Runs the application and returns the process exit code.
pub fn run(cli: Cli) -> ExitCode {
    let mut ctx = match Ctx::new(&cli.global) {
        Ok(ctx) => ctx,
        Err(error) => {
            let mut ui = build_ui(&cli.global);
            return fail(error, &mut ui);
        }
    };
    match dispatch(cli.command, &mut ctx) {
        Ok(code) => code,
        Err(error) => fail(error, &mut ctx.ui),
    }
}

/// The application context shared by all commands.
pub struct Ctx {
    /// The output facade.
    pub ui: Ui,
    /// The cancellation flag shared by all long running operations.
    pub cancel: Cancellation,
}

impl Ctx {
    /// Builds the context from the global options and installs the interrupt
    /// handler.
    pub fn new(global: &GlobalArgs) -> anyhow::Result<Self> {
        let ui = build_ui(global);
        let cancel = Cancellation::default();
        install_handler(&cancel).context("failed to install the ctrl+c handler")?;
        Ok(Self { ui, cancel })
    }
}

/// A cloneable flag signalling that the current operation should stop.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    /// Requests that the running operations stop.
    pub fn request(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Returns `true` if a stop was requested.
    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

fn build_ui(global: &GlobalArgs) -> Ui {
    let glyphs = if global.ascii {
        Glyphs::Ascii
    } else {
        Glyphs::Unicode
    };
    Ui::stdout(
        global.format,
        global.verbose,
        global.progress,
        Theme::new(glyphs),
        global.color.into(),
    )
}

fn install_handler(cancel: &Cancellation) -> Result<(), CtrlcError> {
    let flag = Arc::clone(&cancel.0);
    let result = ctrlc::set_handler(move || {
        // The first interrupt requests a graceful stop, the second one exits
        // immediately so the process cannot get stuck.
        if flag.swap(true, Ordering::SeqCst) {
            std::process::exit(i32::from(EXIT_INTERRUPT));
        }
    });
    match result {
        Ok(()) | Err(CtrlcError::MultipleHandlers) => Ok(()),
        Err(error) => Err(error),
    }
}

fn dispatch(command: Command, ctx: &mut Ctx) -> anyhow::Result<ExitCode> {
    match command {
        Command::Solve(args) => solve::run(args, ctx),
        Command::Search(args) => search::run(args, ctx),
    }
}

fn fail(error: anyhow::Error, ui: &mut Ui) -> ExitCode {
    if error::is_broken_pipe(&error) {
        return ExitCode::from(error::EXIT_SUCCESS);
    }
    let _ = error::render(&error, ui);
    error::map_error(&error)
}
