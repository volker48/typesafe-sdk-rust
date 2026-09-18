use std::collections::BTreeMap;

use typesafe_sdk::{Client, Content, Field, NoulCriteria, Question, SystemOneRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build()?;

    let questions = BTreeMap::from([
        (
            "is_urgent".into(),
            Question::Noul {
                instructions: Field::Value(Content::from(
                    "Does this support request require a fast response?",
                )),
                criteria: Field::Value(NoulCriteria {
                    r#true: Field::Value(Content::from(
                        "The customer reports a blocked payment or a time-sensitive deadline.",
                    )),
                    r#false: Field::Value(Content::from(
                        "The request is informational or has no stated deadline.",
                    )),
                }),
            },
        ),
        (
            "team".into(),
            Question::Choice {
                instructions: Field::Value(Content::from("Which team should handle this request?")),
                criteria: BTreeMap::from([
                    (
                        "billing".into(),
                        Some(Content::from("Payments, invoices, refunds, or charges.")),
                    ),
                    (
                        "technical".into(),
                        Some(Content::from("Bugs, outages, or integration problems.")),
                    ),
                    (
                        "account".into(),
                        Some(Content::from("Login, profile, or account-access problems.")),
                    ),
                ]),
            },
        ),
        (
            "customer_sentiment".into(),
            Question::Score {
                instructions: Field::Value(Content::from("How frustrated is the customer?")),
                criteria: vec![
                    Content::from("Calm or neutral"),
                    Content::from("Frustrated"),
                    Content::from("Very angry or threatening to leave"),
                ],
            },
        ),
    ]);

    let request = SystemOneRequest::new(
        Content::from(
            "Help! My payment has failed three times today and I need it fixed before payroll.",
        ),
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
