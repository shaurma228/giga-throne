//! Running external tools. Every backend goes through [`Runner`], so tests can
//! record the commands and script their answers instead of touching the desktop.

use std::io;
use std::process::Command;

/// What a finished tool said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The tool exited with status 0.
    pub status_ok: bool,
    /// Standard output, decoded lossily.
    pub stdout: String,
}

/// Runs a program with arguments. No shell is involved.
pub trait Runner: Send + Sync {
    /// Starts `program` with `args` and waits for it.
    ///
    /// An `Err` means the program could not be started at all, such as a missing binary.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<Output>;
}

/// The real runner, backed by [`std::process::Command`].
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<Output> {
        let output = Command::new(program).args(args).output()?;
        Ok(Output {
            status_ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        })
    }
}

/// A runner for tests: records every call and answers from a script.
///
/// Commands without a scripted answer succeed with empty output. A command that
/// was given [`FakeRunner::missing`] fails to start, and [`FakeRunner::failing`]
/// makes it exit with a non-zero status.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct FakeRunner {
    calls: std::sync::Mutex<Vec<Vec<String>>>,
    answers: std::sync::Mutex<std::collections::HashMap<String, Answer>>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
enum Answer {
    Stdout(String),
    Failing,
    Missing,
}

#[cfg(test)]
impl FakeRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scripts the stdout of `line`, such as `"kreadconfig6 --key ProxyType"`.
    /// The key is `program` and the arguments joined by single spaces.
    pub fn answer(&self, line: &str, stdout: &str) -> &Self {
        self.set(line, Answer::Stdout(stdout.to_owned()))
    }

    /// Makes `program` exit with a non-zero status when called with `line`.
    pub fn failing(&self, line: &str) -> &Self {
        self.set(line, Answer::Failing)
    }

    /// Makes `program` fail to start when called with `line`.
    pub fn missing(&self, line: &str) -> &Self {
        self.set(line, Answer::Missing)
    }

    /// Every call so far, as `[program, args...]`.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Every call so far, as one line per call, for compact assertions.
    pub fn call_lines(&self) -> Vec<String> {
        self.calls().iter().map(|call| call.join(" ")).collect()
    }

    fn set(&self, line: &str, answer: Answer) -> &Self {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(line.to_owned(), answer);
        self
    }
}

#[cfg(test)]
impl Runner for FakeRunner {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<Output> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|arg| (*arg).to_owned()));
        let line = call.join(" ");
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call);
        let answer = self
            .answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&line)
            .cloned();
        match answer {
            None => Ok(Output {
                status_ok: true,
                stdout: String::new(),
            }),
            Some(Answer::Stdout(stdout)) => Ok(Output {
                status_ok: true,
                stdout,
            }),
            Some(Answer::Failing) => Ok(Output {
                status_ok: false,
                stdout: String::new(),
            }),
            Some(Answer::Missing) => Err(io::Error::new(io::ErrorKind::NotFound, "no such tool")),
        }
    }
}
