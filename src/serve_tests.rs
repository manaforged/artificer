use super::*;

#[test]
fn tokens_are_random() -> Result<()> {
    let first = make_token()?;
    let second = make_token()?;
    assert_eq!(first.len(), 64);
    assert_ne!(first, second);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    Ok(())
}

#[cfg(unix)]
#[test]
fn control_files_are_private() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("artificer");
    fs::create_dir(&home)?;
    fs::set_permissions(&home, fs::Permissions::from_mode(0o777))?;
    control(&home, "serve.token", b"secret")?;
    assert_eq!(fs::metadata(&home)?.permissions().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(home.join("serve.token"))?.permissions().mode() & 0o777,
        0o600,
    );
    Ok(())
}

#[test]
fn oversized_request_is_rejected() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = std::thread::spawn(move || -> Result<()> {
        let (stream, _) = listener.accept()?;
        handle(stream, Path::new("."), "secret")
    });
    let mut client = TcpStream::connect(addr)?;
    client.write_all(&vec![b'x'; MAX_REQUEST + 1])?;
    client.shutdown(std::net::Shutdown::Write)?;
    let mut response = String::new();
    client.read_to_string(&mut response)?;
    server
        .join()
        .map_err(|_thread| anyhow::anyhow!("server thread must not panic"))??;
    let reply: Reply = serde_json::from_str(&response)?;
    assert!(!reply.ok);
    assert_eq!(reply.err, "request too large");
    Ok(())
}

#[test]
fn connection_limit_is_bounded() {
    let claims: Vec<Connection> = (0..MAX_CONNECTIONS)
        .map(|_| Connection::claim().expect("connection within limit"))
        .collect();
    assert!(Connection::claim().is_none());
    drop(claims);
    assert!(Connection::claim().is_some());
}

#[cfg(not(windows))]
#[test]
fn stop_never_terminates_an_unverified_pid() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    control(
        tmp.path(),
        "serve.pid",
        std::process::id().to_string().as_bytes(),
    )?;
    let error = finish_stop(tmp.path(), Some(std::process::id()), Duration::ZERO)
        .expect_err("a live unverified PID must not be terminated");
    assert!(error.to_string().contains("refusing to terminate"));
    assert!(tmp.path().join("serve.pid").is_file());
    Ok(())
}
