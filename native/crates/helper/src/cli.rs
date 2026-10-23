use crate::{transport::read_frame, worker::probe_main};
use cu_core::Policy;
use cu_protocol::*;
use serde_json::json;
use std::io;

pub(crate) fn run_mode(args: &[String]) -> Result<bool> {
    if args.first().is_some_and(|a| a == "--schema") {
        println!(
            "{}",
            serde_json::to_string_pretty(&schemars::schema_for!(ProtocolTypes)).unwrap()
        );
    } else if args.first().is_some_and(|a| a == "--probe") {
        probe_main()?;
    } else if args.first().is_some_and(|a| a == "--validate-protocol") {
        validate_protocol()?;
    } else {
        #[cfg(target_os = "macos")]
        if args
            .first()
            .is_some_and(|arg| matches!(arg.as_str(), "--permissions" | "--request-permissions"))
        {
            permission_mode(args)?;
            return Ok(true);
        }
        return Ok(false);
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
fn permission_mode(args: &[String]) -> Result<()> {
    let policy = load_policy(&args[1..])?;
    if !policy.desktop
        && args.len() > 1
        && policy.allowed_pids.is_empty()
        && policy.allowed_apps.is_empty()
    {
        return Err(Error::new(
            "permission_denied",
            "Host policy has no approved PIDs or applications",
        ));
    }
    let screen_recording = args.len() == 1 || policy.allow_screenshot;
    let request = args[0] == "--request-permissions";
    let mut status = cu_platform_macos::permissions(request, request && screen_recording)?;
    status["required"] = json!({"accessibility":true,"screen_recording":screen_recording});
    println!("{status}");
    Ok(())
}

fn validate_protocol() -> Result<()> {
    let mut input = io::stdin().lock();
    while let Some(frame) =
        read_frame(&mut input).map_err(|e| Error::new("invalid_frame", e.to_string()))?
    {
        let result = serde_json::from_slice::<Request>(&frame);
        println!("{}", json!({"valid":result.is_ok()}));
    }
    Ok(())
}

pub(crate) fn load_policy(args: &[String]) -> Result<Policy> {
    match args {
        [] => Ok(Policy::desktop()),
        [flag, path] if flag == "--policy" => {
            let metadata =
                std::fs::metadata(path).map_err(|e| Error::new("policy_error", e.to_string()))?;
            if metadata.len() > 65536 {
                return Err(Error::new("policy_error", "Policy exceeds 64 KiB"));
            }
            serde_json::from_slice(
                &std::fs::read(path).map_err(|e| Error::new("policy_error", e.to_string()))?,
            )
            .map_err(|e| Error::new("policy_error", e.to_string()))
        }
        _ => Err(Error::new(
            "invalid_params",
            "Usage: cu-helper [--policy absolute-policy.json]",
        )),
    }
}
