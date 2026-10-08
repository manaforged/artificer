use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::temp_sibling;

const ARGUMENT_FILE: &str = "compiler.args";

pub(crate) struct Response {
    pub(crate) command: Command,
    files: Vec<PathBuf>,
}

impl Response {
    pub(crate) fn new(source: &Command, wrapper_args: usize, out: &Path) -> io::Result<Self> {
        let mut command = Command::new(source.get_program());
        if let Some(cwd) = source.get_current_dir() {
            command.current_dir(cwd);
        }
        for (name, value) in source.get_envs() {
            match value {
                Some(value) => {
                    command.env(name, value);
                }
                None => {
                    command.env_remove(name);
                }
            }
        }
        let mut response = Self {
            command,
            files: Vec::new(),
        };
        let mut bytes = Vec::new();
        for (index, arg) in source.get_args().enumerate() {
            if index < wrapper_args {
                response.command.arg(arg);
                continue;
            }
            match arg
                .to_str()
                .filter(|text| !text.starts_with('@') && !text.contains(['\0', '\r', '\n']))
            {
                Some(text) => {
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.push(b'\n');
                }
                None => {
                    response.argument_file(out, &mut bytes)?;
                    response.command.arg(arg);
                }
            }
        }
        response.argument_file(out, &mut bytes)?;
        Ok(response)
    }

    fn argument_file(&mut self, out: &Path, bytes: &mut Vec<u8>) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let path = temp_sibling(&out.join(ARGUMENT_FILE));
        let path = std::path::absolute(path)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        self.files.push(path.clone());
        file.write_all(bytes)?;
        drop(file);
        let mut arg = OsString::from("@");
        arg.push(path);
        self.command.arg(arg);
        bytes.clear();
        Ok(())
    }
}

impl Drop for Response {
    fn drop(&mut self) {
        for path in &self.files {
            if let Err(error) = fs::remove_file(path) {
                crate::out::err(format!(
                    "warning: cannot remove compiler response file {}: {error}",
                    path.display()
                ));
            }
        }
    }
}
