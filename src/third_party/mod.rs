mod prompt_adapters;
mod response_parsing;

use self::prompt_adapters::{AnthropicPrompt, OpenAiPrompt};
use self::response_parsing::{AnthropicResponse, OpenAiResponse};
use crate::input_processing::is_interactive;
use crate::third_party::response_parsing::OllamaResponse;
use crate::{
    config::{
        api::{Api, ApiConfig},
        prompt::{Message, Prompt},
    },
    input_processing::read_user_input,
};

use log::debug;
use std::io;

pub fn make_api_request(api_config: ApiConfig, prompt: &Prompt) -> io::Result<Message> {
    debug!(
        "Trying to reach {:?} with key {:?}",
        api_config.url, api_config.api_key
    );
    debug!("request content: {:?}", prompt);

    validate_prompt_size(prompt);

    let mut prompt = prompt.clone();

    if prompt.model.is_none() {
        prompt.model = api_config.default_model.clone()
    }

    // currently not compatible with streams
    prompt.stream = Some(false);

    let request = ureq::post(&api_config.url);
    let response_text: String = match prompt.api {
        Api::Ollama => {
            let request = request.set("Content-Type", "application/json");
            let response: OllamaResponse =
                read_response(request.send_json(OpenAiPrompt::from(prompt)))?.into_json()?;
            response.into()
        }
        Api::Openai | Api::Mistral | Api::Groq => {
            let request = request.set("Content-Type", "application/json").set(
                "Authorization",
                &format!("Bearer {}", &api_config.get_api_key()),
            );
            let response: OpenAiResponse =
                read_response(request.send_json(OpenAiPrompt::from(prompt)))?.into_json()?;
            response.into()
        }
        Api::Anthropic => {
            let request = request
                .set("Content-Type", "application/json")
                .set("x-api-key", &api_config.get_api_key())
                .set(
                    "anthropic-version",
                    &api_config.version.expect(
                        "version required for Anthropic, please add version key to your api config",
                    ),
                );
            let response: AnthropicResponse =
                read_response(request.send_json(AnthropicPrompt::from(prompt)))?.into_json()?;
            response.into()
        }
        Api::AnotherApiForTests => panic!("This api is not made for actual use."),
    };

    Ok(Message::assistant(&response_text))
}

fn read_response(response: Result<ureq::Response, ureq::Error>) -> io::Result<ureq::Response> {
    response.map_err(|e| match e {
        ureq::Error::Status(status, response) => {
            let content = match response.into_string() {
                Ok(content) => content,
                Err(_) => "(non-UTF-8 response)".to_owned(),
            };
            io::Error::other(format!(
                "API call failed with status code {status} and body: {content}"
            ))
        }
        ureq::Error::Transport(transport) => io::Error::other(transport),
    })
}

fn validate_prompt_size(prompt: &Prompt) {
    let char_limit = prompt.char_limit.unwrap_or_default();
    let number_of_chars: u32 = prompt
        .messages
        .iter()
        .map(|message| message.content.len() as u32)
        .sum();

    debug!("Number of chars is prompt: {}", number_of_chars);

    if char_limit > 0 && number_of_chars > char_limit {
        if is_interactive() {
            println!(
                "The number of chars in the input {} is greater than the set limit {}\n\
                Do you want to continue? High costs may ensue.\n[Y/n]",
                number_of_chars, char_limit,
            );
            let input = read_user_input();
            if input.trim() != "Y" {
                println!("exiting...");
                std::process::exit(0);
            }
        } else {
            panic!(
                "Input {} larger than limit {} in non-interactive mode. Exiting.",
                number_of_chars, char_limit
            );
        }
    }
}
