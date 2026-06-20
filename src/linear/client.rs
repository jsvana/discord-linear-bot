use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct LinearClient {
    client: Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct LinearIssue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug)]
pub struct LinearComment {
    pub id: String,
    pub body: String,
    pub created_at: String,
    pub author_name: String,
}

#[derive(Debug)]
pub struct LinearIssueStatus {
    pub id: String,
    pub identifier: String,
    pub status_name: String,
    pub status_type: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadFile {
    pub upload_url: String,
    pub asset_url: String,
    pub headers: Vec<UploadHeader>,
}

#[derive(Debug, Deserialize)]
pub struct UploadHeader {
    pub key: String,
    pub value: String,
}

impl LinearClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    pub async fn create_issue(
        &self,
        team_id: &str,
        title: &str,
        description: &str,
        label_ids: &[String],
        project_id: &str,
    ) -> Result<LinearIssue, AppError> {
        let query = r#"
            mutation CreateIssue($input: IssueCreateInput!) {
                issueCreate(input: $input) {
                    success
                    issue {
                        id
                        identifier
                        title
                        url
                    }
                }
            }
        "#;

        let variables = json!({
            "input": {
                "teamId": team_id,
                "title": title,
                "description": description,
                "labelIds": label_ids,
                "projectId": project_id,
            }
        });

        let data = self.execute(query, variables).await?;
        let issue_data = &data["issueCreate"]["issue"];

        Ok(LinearIssue {
            id: issue_data["id"]
                .as_str()
                .ok_or_else(|| AppError::LinearApi("Missing issue id".into()))?
                .to_string(),
            identifier: issue_data["identifier"]
                .as_str()
                .ok_or_else(|| AppError::LinearApi("Missing issue identifier".into()))?
                .to_string(),
            title: issue_data["title"]
                .as_str()
                .ok_or_else(|| AppError::LinearApi("Missing issue title".into()))?
                .to_string(),
            url: issue_data["url"]
                .as_str()
                .ok_or_else(|| AppError::LinearApi("Missing issue url".into()))?
                .to_string(),
        })
    }

    /// Fetch issues updated since `since` (ISO 8601 timestamp) for a specific team.
    pub async fn get_updated_issues(
        &self,
        team_id: &str,
        since: &str,
    ) -> Result<Vec<LinearIssueStatus>, AppError> {
        let query = r#"
            query UpdatedIssues($teamId: ID!, $since: DateTimeOrDuration!) {
                issues(
                    filter: {
                        team: { id: { eq: $teamId } }
                        updatedAt: { gt: $since }
                    }
                    first: 100
                ) {
                    nodes {
                        id
                        identifier
                        state {
                            name
                            type
                        }
                        updatedAt
                    }
                }
            }
        "#;

        let variables = json!({
            "teamId": team_id,
            "since": since,
        });

        let data = self.execute(query, variables).await?;
        let nodes = data["issues"]["nodes"]
            .as_array()
            .ok_or_else(|| AppError::LinearApi("Missing issues.nodes".into()))?;

        let mut results = Vec::new();
        for node in nodes {
            let id = node["id"].as_str().unwrap_or_default().to_string();
            let identifier = node["identifier"].as_str().unwrap_or_default().to_string();
            let status_name = node["state"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let status_type = node["state"]["type"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let updated_at = node["updatedAt"].as_str().unwrap_or_default().to_string();

            results.push(LinearIssueStatus {
                id,
                identifier,
                status_name,
                status_type,
                updated_at,
            });
        }

        Ok(results)
    }

    /// Fetch current state for a specific set of issue IDs in a single query.
    /// Issues that no longer exist or aren't visible are silently omitted from the result.
    pub async fn get_issues_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<LinearIssueStatus>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let query = r#"
            query IssuesByIds($ids: [ID!]!) {
                issues(filter: { id: { in: $ids } }, first: 250) {
                    nodes {
                        id
                        identifier
                        state {
                            name
                            type
                        }
                        updatedAt
                    }
                }
            }
        "#;

        let variables = json!({ "ids": ids });
        let data = self.execute(query, variables).await?;
        let nodes = data["issues"]["nodes"]
            .as_array()
            .ok_or_else(|| AppError::LinearApi("Missing issues.nodes".into()))?;

        let mut results = Vec::new();
        for node in nodes {
            let id = node["id"].as_str().unwrap_or_default().to_string();
            let identifier = node["identifier"].as_str().unwrap_or_default().to_string();
            let status_name = node["state"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let status_type = node["state"]["type"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let updated_at = node["updatedAt"].as_str().unwrap_or_default().to_string();

            results.push(LinearIssueStatus {
                id,
                identifier,
                status_name,
                status_type,
                updated_at,
            });
        }

        Ok(results)
    }

    /// Fetch comments for a specific issue, sorted by creation time.
    pub async fn get_issue_comments(&self, issue_id: &str) -> Result<Vec<LinearComment>, AppError> {
        let query = r#"
            query IssueComments($issueId: String!) {
                issue(id: $issueId) {
                    comments(first: 100, orderBy: createdAt) {
                        nodes {
                            id
                            body
                            createdAt
                            user {
                                displayName
                            }
                        }
                    }
                }
            }
        "#;

        let variables = json!({
            "issueId": issue_id,
        });

        let data = self.execute(query, variables).await?;
        let nodes = data["issue"]["comments"]["nodes"]
            .as_array()
            .ok_or_else(|| AppError::LinearApi("Missing issue.comments.nodes".into()))?;

        let mut results = Vec::new();
        for node in nodes {
            let id = node["id"].as_str().unwrap_or_default().to_string();
            let body = node["body"].as_str().unwrap_or_default().to_string();
            let created_at = node["createdAt"].as_str().unwrap_or_default().to_string();
            let author_name = node["user"]["displayName"]
                .as_str()
                .unwrap_or("Unknown")
                .to_string();

            results.push(LinearComment {
                id,
                body,
                created_at,
                author_name,
            });
        }

        Ok(results)
    }

    pub async fn request_file_upload(
        &self,
        filename: &str,
        content_type: &str,
        size: u64,
    ) -> Result<UploadFile, AppError> {
        let query = r#"
            mutation FileUpload($contentType: String!, $filename: String!, $size: Int!) {
                fileUpload(contentType: $contentType, filename: $filename, size: $size) {
                    uploadFile {
                        uploadUrl
                        assetUrl
                        headers {
                            key
                            value
                        }
                    }
                }
            }
        "#;

        let variables = json!({
            "contentType": content_type,
            "filename": filename,
            "size": size,
        });

        let data = self.execute(query, variables).await?;
        let upload_data = &data["fileUpload"]["uploadFile"];

        let headers: Vec<UploadHeader> = serde_json::from_value(upload_data["headers"].clone())
            .map_err(|e| AppError::LinearApi(format!("Failed to parse upload headers: {e}")))?;

        Ok(UploadFile {
            upload_url: upload_data["uploadUrl"]
                .as_str()
                .ok_or_else(|| AppError::LinearApi("Missing uploadUrl".into()))?
                .to_string(),
            asset_url: upload_data["assetUrl"]
                .as_str()
                .ok_or_else(|| AppError::LinearApi("Missing assetUrl".into()))?
                .to_string(),
            headers,
        })
    }

    pub async fn upload_file_to_url(
        &self,
        upload: &UploadFile,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<String, AppError> {
        let mut request = self
            .client
            .put(&upload.upload_url)
            .header("Content-Type", content_type);

        for header in &upload.headers {
            request = request.header(&header.key, &header.value);
        }

        let response = request
            .body(data)
            .send()
            .await
            .map_err(|e| AppError::AttachmentUpload(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::AttachmentUpload(format!(
                "Upload returned {status}: {body}"
            )));
        }

        debug!(asset_url = %upload.asset_url, "File uploaded to Linear");
        Ok(upload.asset_url.clone())
    }

    pub async fn download_attachment(&self, url: &str) -> Result<(Vec<u8>, String), AppError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::AttachmentUpload(format!("Download failed: {e}")))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::AttachmentUpload(format!("Failed to read body: {e}")))?;

        Ok((bytes.to_vec(), content_type))
    }

    async fn execute(&self, query: &str, variables: Value) -> Result<Value, AppError> {
        #[derive(Serialize)]
        struct GraphQLRequest<'a> {
            query: &'a str,
            variables: &'a Value,
        }

        #[derive(Deserialize)]
        struct GraphQLResponse {
            data: Option<Value>,
            errors: Option<Vec<GraphQLError>>,
        }

        #[derive(Deserialize)]
        struct GraphQLError {
            message: String,
        }

        // Transient failures (host DNS blips, Linear edge 5xx, rate limits) are
        // retried with exponential backoff. Non-retryable errors (4xx other than
        // 429, GraphQL-level errors) fail immediately.
        const MAX_ATTEMPTS: u32 = 3;
        let body = GraphQLRequest {
            query,
            variables: &variables,
        };

        let mut attempt = 0;
        let text = loop {
            attempt += 1;

            let send_result = self
                .client
                .post("https://api.linear.app/graphql")
                .header("Authorization", &self.api_key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            let response = match send_result {
                Ok(response) => response,
                Err(e) => {
                    if attempt < MAX_ATTEMPTS {
                        let delay = backoff(attempt);
                        warn!(
                            attempt,
                            error = %e,
                            delay_secs = delay.as_secs(),
                            "Linear request failed to send, retrying"
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let status = response.status();

            // 5xx and 429 are worth retrying; everything else is decided now.
            if (status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
                && attempt < MAX_ATTEMPTS
            {
                let delay = backoff(attempt);
                warn!(
                    attempt,
                    %status,
                    delay_secs = delay.as_secs(),
                    "Linear returned retryable status, retrying"
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            let text = response.text().await?;

            if !status.is_success() {
                let snippet: String = text.chars().take(500).collect();
                return Err(AppError::LinearApi(format!("HTTP {status}: {snippet}")));
            }

            break text;
        };

        let response: GraphQLResponse = serde_json::from_str(&text).map_err(|e| {
            let snippet: String = text.chars().take(500).collect();
            AppError::LinearApi(format!(
                "Failed to decode response body: {e}; body: {snippet}"
            ))
        })?;

        if let Some(errors) = response.errors {
            let messages: Vec<String> = errors.into_iter().map(|e| e.message).collect();
            let combined = messages.join("; ");
            warn!(errors = %combined, "Linear API returned errors");
            return Err(AppError::LinearApi(combined));
        }

        response
            .data
            .ok_or_else(|| AppError::LinearApi("No data in response".into()))
    }
}

/// Exponential backoff for retry attempt N (1-indexed): 1s, 2s, 4s, ...
fn backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(2u64.pow(attempt.saturating_sub(1)))
}
