use super::*;

pub(super) fn handle(stream: TcpStream, home: &Path, token: &str) -> Result<()> {
    stream.set_read_timeout(Some(IO))?;
    stream.set_write_timeout(Some(IO))?;
    let mut reader = BufReader::new(stream);
    let reply = answer(&mut reader, home, token)?;
    write_reply(reader.get_mut(), &reply)
}

fn answer(reader: &mut BufReader<TcpStream>, home: &Path, token: &str) -> Result<Reply> {
    let mut line = Vec::new();
    reader
        .by_ref()
        .take((MAX_REQUEST + 1) as u64)
        .read_until(b'\n', &mut line)?;
    if line.len() > MAX_REQUEST || line.last() != Some(&b'\n') {
        return Ok(Reply::fail("request too large"));
    }
    let req: Request = serde_json::from_slice(&line).context("serve request")?;
    if req.token != token {
        return Ok(Reply::fail("bad token"));
    }
    match req.op.as_str() {
        "ping" => return Ok(Reply::ok(0, String::new(), "pong\n".into())),
        "quit" => {
            control(home, "serve.stop", b"")?;
            return Ok(Reply::ok(0, String::new(), String::new()));
        }
        _ => {}
    }
    drop(reader.get_ref().set_read_timeout(None));
    let (result, stdout, stderr) = crate::out::capture(|| dispatch(home, &req));
    Ok(match result {
        Ok(code) => Reply::ok(code, stdout, stderr),
        Err(e) => Reply::error(&e, stdout, stderr),
    })
}

fn dispatch(home: &Path, req: &Request) -> Result<i32> {
    match req.op.as_str() {
        "check" | "warm" | "build" => crate::check_cmd(
            &req.dir,
            &req.packages,
            home,
            crate::CheckOpts {
                json: req.json,
                workspace: req.workspace || req.op == "warm",
                all_features: req.all_features,
                features: req.features.clone(),
                no_default: req.no_default,
                meta_flags: req.meta_flags.clone(),
                target_dir: req.target_dir.clone(),
                release: req.release,
                link: req.link || req.op == "build",
                targets: crate::Targets {
                    tests: req.tests,
                    all: req.all_targets,
                },
            },
        ),
        "test" => crate::test_package(
            &req.dir,
            &req.packages,
            home,
            &crate::TestOpts {
                no_run: req.no_run,
                json: req.json,
                workspace: req.workspace,
                all_features: req.all_features,
                features: req.features.clone(),
                no_default: req.no_default,
                meta_flags: req.meta_flags.clone(),
                target_dir: req.target_dir.clone(),
                release: req.release,
                lib: req.lib,
                doc: req.doc,
                only: req.only.clone(),
                args: req.args.clone(),
            },
        ),
        other => bail!("unknown op {other}"),
    }
}

pub(super) fn write_reply(stream: &mut TcpStream, reply: &Reply) -> Result<()> {
    let line = serde_json::to_string(reply)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

impl Reply {
    fn ok(code: i32, stdout: String, stderr: String) -> Self {
        Self {
            ok: true,
            code,
            stdout,
            stderr,
            err: String::new(),
        }
    }

    fn error(e: &anyhow::Error, stdout: String, stderr: String) -> Self {
        let code = if e.is::<crate::cargo::Unmodeled>() {
            2
        } else {
            101
        };
        let err = if e.is::<crate::out::Reported>() {
            String::new()
        } else {
            e.to_string()
        };
        Self {
            ok: false,
            code,
            stdout,
            stderr,
            err,
        }
    }

    pub(super) fn fail(err: &str) -> Self {
        Self {
            ok: false,
            code: 1,
            stdout: String::new(),
            stderr: String::new(),
            err: err.into(),
        }
    }
}
