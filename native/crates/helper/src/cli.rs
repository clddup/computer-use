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
        if args.len() == 1 && matches!(args[0].as_str(), "--permissions" | "--request-permissions")
        {
            println!(
                "{}",
                cu_platform_macos::permissions(args[0] == "--request-permissions")?
            );
            return Ok(true);
        }
        return Ok(false);
    }
    Ok(true)
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
        [] => Ok(Policy::default()),
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
