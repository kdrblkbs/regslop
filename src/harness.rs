use std::path::Path;
use std::time::Duration;

use reqwest::blocking::Client;

use crate::BedrockConfig;
use crate::converse;
use crate::tools;

fn build_request_url(base: &str, model: &str) -> String {
    format!("{base}/model/{model}/converse")
}

fn build_request(
    system_prompt: &str,
    messages: Vec<converse::Message>,
    max_tokens: u32,
) -> converse::Request {
    let tools = tools::get_all()
        .into_iter()
        .map(|(name, tool)| {
            converse::Tool::ToolSpec(converse::ToolSpec {
                name,
                description: tool.description,
                input_schema: converse::InputSchema::Json(tool.schema),
            })
        })
        .collect::<Vec<converse::Tool>>();
    converse::Request {
        additional_model_request_fields: None,
        inference_config: Some(converse::InferenceConfig {
            max_tokens: Some(max_tokens),
            temperature: Some(0.1),
            top_p: None,
            stop_sequences: Vec::new(),
        }),
        system: vec![converse::SystemBlock::Text(system_prompt.to_string())],
        messages,
        tool_config: Some(converse::ToolConfig {
            tools,
            tool_choice: None,
        }),
    }
}

fn ensure_is_success_response(
    response: reqwest::blocking::Response,
) -> anyhow::Result<reqwest::blocking::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let error_type = response
        .headers()
        .get("x-amzn-errortype")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(':').next())
        .unwrap_or("unknown error type")
        .to_owned();
    let body = response
        .text()
        .unwrap_or_else(|e| format!("failed to read response body: {e}"));
    anyhow::bail!("request failed with: {status} - {error_type} - {body}");
}

fn validate_response_message(message: &converse::Message) -> anyhow::Result<()> {
    if message.role != converse::Role::Assistant {
        anyhow::bail!("response message has wrong role: {message:?}")
    }
    for content in &message.content {
        match content {
            converse::ContentBlock::Text(_)
            | converse::ContentBlock::ToolUse(_)
            | converse::ContentBlock::CachePoint(_) => {}
            converse::ContentBlock::ToolResult(result) => anyhow::bail!(
                "assistant message contains a toolResult for tool_use_id '{}', only the harness sends tool results",
                result.tool_use_id
            ),
            converse::ContentBlock::ReasoningContent(reasoning) => {
                warn_if_reasoning_is_unknown(reasoning);
            }
            converse::ContentBlock::Unknown(u) => {
                if u.get("toolUse").is_some() {
                    anyhow::bail!("failed to parse toolUse: {u}");
                }
                eprintln!("WARN: content block is unknown {u}");
            }
        }
    }
    Ok(())
}

fn warn_if_reasoning_is_unknown(reasoning: &converse::Reasoning) {
    if let converse::Reasoning::Unknown(u) = reasoning {
        eprintln!("WARN: reasoning is unknown {u}");
    }
}

fn log_request(request: &converse::Request) {
    // TODO: WTF?!?!?
    println!("{request:#?}");
}

fn log_response(response: &converse::Response) {
    // TODO: WTF?!?!?
    println!("{response:#?}");
}

fn run_tool_use(sandbox_root: &Path, tool_use: &converse::ToolUse) -> converse::ContentBlock {
    let (text, status) = match tools::run(sandbox_root, &tool_use.name, tool_use.input.clone()) {
        Ok(output) => (output, converse::ToolStatus::Success),
        Err(e) => (format!("{e:#}"), converse::ToolStatus::Error),
    };
    converse::ContentBlock::ToolResult(converse::ToolResult {
        tool_use_id: tool_use.tool_use_id.clone(),
        content: vec![converse::ToolResultContent::Text(text)],
        status: Some(status),
    })
}

fn run_tool_uses(
    sandbox_root: &Path,
    message: &converse::Message,
) -> anyhow::Result<Vec<converse::ContentBlock>> {
    let tool_results: Vec<converse::ContentBlock> = message
        .content
        .iter()
        .filter_map(|content| match content {
            converse::ContentBlock::ToolUse(tool_use) => Some(run_tool_use(sandbox_root, tool_use)),
            _ => None,
        })
        .collect();
    if tool_results.is_empty() {
        anyhow::bail!(
            "stop reason is tool use, but the message contains no toolUse block: {:?}",
            message.content
        );
    }
    Ok(tool_results)
}

pub(crate) fn run(sandbox_root: &Path, bedrock_config: &BedrockConfig) -> anyhow::Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_mins(bedrock_config.client_timeout_mins))
        .build()?;

    let request_url = build_request_url(&bedrock_config.base_url, &bedrock_config.model);
    let task_message = converse::Message {
        role: converse::Role::User,
        content: vec![converse::ContentBlock::Text(
            bedrock_config.task_prompt.clone(),
        )],
    };
    let mut request = build_request(
        &bedrock_config.system_prompt,
        vec![task_message],
        bedrock_config.max_tokens,
    );

    for i in 0..bedrock_config.max_iterations {
        println!("iteration: {i}");
        log_request(&request);

        let http_response = client
            .post(&request_url)
            .bearer_auth(&bedrock_config.token)
            .json(&request)
            .send()?;

        let http_response = ensure_is_success_response(http_response)?;
        // TODO: if this fails i have no clue whats going wrong
        // to text first and then parse, so i still have text
        // but currently i am to lazy
        let response: converse::Response = http_response.json()?;
        log_response(&response);
        let message_from_response = response.output.into_message()?;
        validate_response_message(&message_from_response)?;
        let tool_results = match response.stop_reason {
            converse::StopReason::EndTurn => return Ok(()),
            converse::StopReason::ToolUse => run_tool_uses(sandbox_root, &message_from_response)?,
            _ => anyhow::bail!("not implemented StopReason: {:?}", response.stop_reason),
        };
        request.messages.push(message_from_response);
        request.messages.push(converse::Message {
            role: converse::Role::User,
            content: tool_results,
        });
        println!("########################################################################");
    }

    anyhow::bail!(
        "model did not end its turn within {} iterations, the task is likely unfinished",
        bedrock_config.max_iterations
    );
}
