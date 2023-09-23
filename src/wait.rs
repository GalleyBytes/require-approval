use std::fs::metadata;
use std::process::exit;
use std::time::Duration;
use std::{env, thread};

pub fn wait() {
    let generation_path = env::var("TFO_GENERATION_PATH").expect("TFO_GENERATION_PATH is not set");
    let pod_uid = env::var("POD_UID").expect("POD_UID is not set");
    loop {
        let approval = format!("{}/_approved_{}", generation_path, pod_uid);
        let cancelation = format!("{}/_canceled_{}", generation_path, pod_uid);

        let approved = metadata(approval);
        match approved {
            Ok(_) => {
                println!("Workflow was approved");
                exit(0)
            }
            Err(_) => "",
        };

        let canceled = metadata(cancelation);
        match canceled {
            Ok(_) => {
                println!("Workflow was canceled");
                exit(1)
            }
            Err(_) => "",
        };

        thread::sleep(Duration::from_secs(1));
    }
}
