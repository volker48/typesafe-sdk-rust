use serde_json::json;
use typesafe_sdk::{Client, SystemOneRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build()?;

    let questions = json!({
        "is_urgent": {
            "type": "noul",
            "instructions": "Does this support request require a fast response?",
            "criteria": {
                "true": "The customer reports a blocked payment or a time-sensitive deadline.",
                "false": "The request is informational or has no stated deadline."
            }
        },
        "team": {
            "type": "choice",
            "instructions": "Which team should handle this request?",
            "criteria": {
                "billing": "Payments, invoices, refunds, or charges.",
                "technical": "Bugs, outages, or integration problems.",
                "account": "Login, profile, or account-access problems."
            }
        },
        "customer_sentiment": {
            "type": "score",
            "instructions": "How frustrated is the customer?",
            "criteria": [
                "Calm or neutral",
                "Frustrated",
                "Very angry or threatening to leave"
            ]
        }
    });

    let request = SystemOneRequest::from_json(
        json!("Help! My payment has failed three times today and I need it fixed before payroll."),
        questions,
    )?;
    let response = client.system_one(&request).await?;

    println!("Model: {}", response.data.model);
    println!("Usage: {:?}", response.data.usage);
    println!("HTTP status: {}", response.metadata.status);
    println!("Request ID: {:?}", response.metadata.request_id());
    println!("Answers:");
    for (name, answer) in &response.data.answers {
        println!("  {name}: {answer:?}");
    }

    Ok(())
}
