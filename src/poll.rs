use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Read;
use std::thread;
use std::time::Duration;
use std::{env, process::exit};

#[derive(Deserialize, Debug)]
struct GenericTfoApiResponse {
    //status_info: StatusInfo,
    data: Option<Value>,
}

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
    refresh_url: String,
    token: String,
    token_path: String,
    refresh_token_path: String,
}

impl APIClient {
    fn new(
        url: String,
        refresh_url: String,
        token: String,
        token_path: String,
        refresh_token_path: String,
    ) -> APIClient {
        APIClient {
            url,
            refresh_url,
            token,
            token_path,
            refresh_token_path,
        }
    }

    fn read_refresh_token(&self) -> String {
        read_file(&self.refresh_token_path).expect("Could not read file")
    }

    #[tokio::main]
    async fn query_approval(&mut self) -> Result<String, Option<reqwest::Error>> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "TOKEN",
            reqwest::header::HeaderValue::from_str(self.token.as_str()).unwrap(),
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()?;

        let response = client
            .get(&self.url)
            .headers(headers.clone())
            .send()
            .await?;

        if response.status() == StatusCode::UNAUTHORIZED {
            // The token is expired or invalid
            let mut refresh_attempts = 0;
            let max_refresh_attempts = 6;
            loop {
                if refresh_attempts > max_refresh_attempts {
                    break;
                }
                match self.get_new_token().await {
                    Ok(token) => {
                        println!("INFO new token found");
                        self.token = token;
                        return Ok("NEW_TOKEN".into());
                    }
                    Err(err) => {
                        println!("ERROR {}", err);
                        refresh_attempts += 1;
                    }
                }
                thread::sleep(Duration::from_secs(15));
            }
        }

        let body = response.text().await?;
        Ok(body)
    }

    /// First read token from disk. If unchanged, read the refresh_token from disk and request a new token.
    async fn get_new_token(&self) -> Result<String, Box<dyn std::error::Error>> {
        if self.token.trim() != read_file(&self.token_path)?.trim() {
            return Ok(read_file(&self.token_path)?);
        }

        let client = reqwest::Client::new();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Token",
            reqwest::header::HeaderValue::from_str(self.token.as_str()).unwrap(),
        );

        let refresh_token = self.read_refresh_token();
        let response = client
            .post(&self.refresh_url)
            .headers(headers)
            .json(&json!({"refresh_token": refresh_token}))
            .send()
            .await?;
        if response.status() != StatusCode::OK {
            return Err(response.text().await?.into());
        } else {
            let api_response = response.json::<GenericTfoApiResponse>().await?;
            let data = api_response
                .data
                .expect("Refresh token response did not contain new JWT");
            let arr = data
                .as_array()
                .expect("Refresh token repsosne was not properly formatted");
            if arr.len() != 1 {
                return Err("Refresh token response did not contain data".into());
            }
            let new_token = arr
                .get(0)
                .unwrap()
                .as_str()
                .expect("Refresh token data was not properly formatted");
            return Ok(new_token.to_string());
        }
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

fn read_file(filepath: &str) -> std::io::Result<String> {
    let mut f = File::open(filepath)?;
    let mut buffer = String::new();
    f.read_to_string(&mut buffer)?;
    Ok(buffer)
}

pub fn poll() {
    let url = env::var("TFO_API_URL").unwrap_or_default();
    let token = env::var("TFO_API_LOG_TOKEN").unwrap_or_default();
    let generation_path = env::var("TFO_GENERATION_PATH").expect("TFO_GENERATION_PATH");
    let pod_uid = env::var("POD_UID").expect("POD_UID");
    let token_path =
        env::var("TFO_API_TOKEN_PATH").unwrap_or(String::from("/jwt/TFO_API_LOG_TOKEN"));
    let refresh_token_path =
        env::var("TFO_API_REFRESH_TOKEN_PATH").unwrap_or(String::from("/jwt/REFRESH_TOKEN"));

    if url == String::from("") {
        println!("TFO_API_URL missing: skipping API approval-status check");
        return;
    }

    if token == String::from("") {
        println!("TFO_API_LOG_TOKEN missing: skipping API approval-status check");
        return;
    }

    let mut client = APIClient::new(
        format!("{}/api/v1/task/{}/approval-status", url, pod_uid),
        format!("{}/refresh", url),
        token,
        token_path,
        refresh_token_path,
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
