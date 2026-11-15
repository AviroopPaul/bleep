//! Live check of the API adapters (not run in CI). Needs the provider's key in the env.
//! `cargo run --example api_smoke -- <model> [openrouter|gemini|groq]`
use bleep_lib::runtime::api::adapter::ModelAdapter;
use bleep_lib::runtime::api::openai::OpenAiAdapter;
use bleep_lib::runtime::api::types::*;
use futures::StreamExt;
use serde_json::json;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (var, base) = match args.get(2).map(String::as_str) {
        Some("gemini") => ("GOOGLE_API_KEY", "https://generativelanguage.googleapis.com/v1beta/openai"),
        Some("groq") => ("GROQ_API_KEY", "https://api.groq.com/openai/v1"),
        _ => ("OPENROUTER_API_KEY", "https://openrouter.ai/api/v1"),
    };
    let key = std::env::var(var).expect(var);
    let model = args.get(1).cloned().unwrap_or("google/gemini-2.5-flash".into());
    let a = OpenAiAdapter::new(base, &key, false);
    let list = a.list_models().await.expect("models");
    let m = list.iter().find(|m| m.id == model);
    println!("models: {} listed; {model}: window {:?}, price {:?}", list.len(), m.and_then(|m| m.window), m.and_then(|m| m.price));
    let tool = ToolDef { name: "bleep_now".into(), description: "The current date and time.".into(), schema: json!({"type": "object", "properties": {"tz": {"type": "string"}}}) };
    for (label, msgs, choice) in [
        ("text", vec![Msg::user_text("Reply with the single word OK.")], ToolChoice::Auto),
        ("tool", vec![Msg::user_text("What time is it in Asia/Kolkata? Use the tool.")], ToolChoice::Auto),
        ("forced", vec![Msg::user_text("Is 'Your invoice is overdue' urgent?")], ToolChoice::Force("bleep_now".into())),
    ] {
        let req = ModelRequest { model: model.clone(), system: "Be brief.".into(), msgs, tools: vec![tool.clone()], choice, max_tokens: 100 };
        let ev: Vec<_> = a.stream(&req).await.expect("stream").collect().await;
        println!("{label}: {ev:?}");
    }
}
