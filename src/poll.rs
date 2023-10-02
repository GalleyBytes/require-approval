use std::{env, process::exit};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct Response {
    status_info: StatusInfo,
    data: Vec<DataItem>,
}

impl Response {
    fn is_status_ok(&self) -> bool {
        self.status_info.status_code == 200
    }

    fn is_unauthorized(&self) -> bool {
        self.status_info.status_code == 401
    }

    fn is_nodata(&self) -> bool {
        if self.data.len() == 0 {
            return true;
        }
        if self.data[0].status != "complete" {
            return true;
        }
        false
    }

    fn is_approved(&self) -> bool {
        if self.is_nodata() {
            return false;
        }
        self.data[0].is_approved
    }
}

#[derive(Debug, Default, Deserialize)]
struct StatusInfo {
    status_code: u16,
    // message: String,
}

#[derive(Debug, Deserialize)]
struct DataItem {
    status: String,
    is_approved: bool,
}

#[derive(Debug)]
struct APIClient {
    url: String,
    token: String,
}

impl APIClient {
    fn new(url: String, token: String) -> APIClient {
        APIClient { url, token }
    }

    #[tokio::main]
    async fn query_approval(&self) -> Result<String, Option<reqwest::Error>> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "TOKEN",
            reqwest::header::HeaderValue::from_str(self.token.as_str()).unwrap(),
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()?;

        let body = client
            .get(&self.url)
            .headers(headers)
            .send()
            .await?
            .text()
            .await?;
        Ok(body)
    }
}

/// Return int where: 0=false, 1=true and anything else is nodata
fn response_check(resp: String) -> i8 {
    let r: Response = serde_json::from_str(resp.as_str()).unwrap_or(Response::default());
    if r.is_unauthorized() {
        println!("The request was unauthorized");
        exit(1);
    }
    if !r.is_status_ok() {
        return -1;
    }
    if r.is_nodata() {
        return -1;
    }
    return r.is_approved() as i8;
}

pub fn poll() {
    let url = env::var("TFO_API_URL").unwrap_or_default();
    let token = env::var("TFO_API_LOG_TOKEN").unwrap_or_default();
    let generation_path = env::var("TFO_GENERATION_PATH").expect("TFO_GENERATION_PATH");
    let pod_uid = env::var("POD_UID").expect("POD_UID");

    if url == String::from("") {
        println!("TFO_API_URL missing: skipping API approval-status check");
        return;
    }

    if token == String::from("") {
        println!("TFO_API_LOG_TOKEN missing: skipping API approval-status check");
        return;
    }

    let client = APIClient::new(
        format!("{}/api/v1/task/{}/approval-status", url, pod_uid),
        token,
    );
    loop {
        let response = client.query_approval();
        let response_data = match response {
            Ok(s) => s,
            Err(e) => {
                let err = e.unwrap();
                println!("{}", err.to_string());
                String::from("")
            }
        };
        let status = response_check(response_data);
        match status {
            0 => {
                println!("Canceled via database");
                let _ = std::fs::write(format!("{}/_canceled_{}", generation_path, pod_uid), "");
                exit(1)
            }
            1 => {
                println!("Approved via database");
                let _ = std::fs::write(format!("{}/_approved_{}", generation_path, pod_uid), "");
                exit(0)
            }
            _ => {
                println!("...waiting for approval result in database")
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
