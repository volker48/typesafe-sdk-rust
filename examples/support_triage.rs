//! Synthetic support triage. Run with TYPESAFE_API_KEY set:
//! `cargo run --example support_triage` (performs two inference calls).
use serde_json::json;
use std::time::Duration;
use typesafe_sdk::{
    Answer, Client, RequestOptions, RetryPolicy, SystemOneRequest, SystemOneResponse,
};

const CRITERIA_VERSION: &str = "support-triage-v1";

#[derive(Debug, PartialEq)]
enum Decision {
    EscalateBilling,
    EscalateTechnical,
    Review,
}

fn decide(response: &SystemOneResponse) -> Decision {
    // Illustrative application policy, not a calibrated SDK default. Keep the
    // review branch until thresholds have been evaluated on representative cases.
    match (response.answers.get("urgent"), response.answers.get("team")) {
        (Some(Answer::Noul(urgent)), Some(Answer::Choice(team)))
            if (0.85..=1.0).contains(&urgent.noul) && (0.85..=1.0).contains(&team.confidence) =>
        {
            match team.choice.as_str() {
                "billing" => Decision::EscalateBilling,
                "technical" => Decision::EscalateTechnical,
                _ => Decision::Review,
            }
        }
        _ => Decision::Review,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build()?;
    let questions = json!({
        "urgent": {
            "type": "noul",
            "instructions": "Does this request need an urgent response?",
            "criteria": {
                "true": "A blocked payment or outage with a deadline.",
                "false": "An informational question with no deadline."
            }
        },
        "team": {
            "type": "choice",
            "instructions": "Which team should handle the request?",
            "criteria": {"billing": "Payments and invoices", "technical": "Bugs and outages"}
        }
    });
    let options = RequestOptions {
        timeout: Some(Duration::from_secs(5)), // Limit each attempt.
        retry: Some(RetryPolicy::disabled()),  // Avoid automatically repeating inference.
        ..Default::default()
    };
    // Keep these case IDs and criteria version with application evaluation results.
    for (case_id, message) in [
        (
            "payroll",
            "My payment failed three times and payroll is due today.",
        ),
        ("invoice-help", "Where can I download last month's invoice?"),
    ] {
        let request = SystemOneRequest::from_json(json!({"message": message}), questions.clone())?;
        // The caller's total deadline includes any future retry configuration.
        // Cancellation does not prove the server stopped processing a request.
        let response = tokio::time::timeout(
            Duration::from_secs(8),
            client.system_one_with(&request, &options),
        )
        .await??;
        println!(
            "case={case_id} criteria={CRITERIA_VERSION} model={} usage={:?} request_id={:?} decision={:?}",
            response.data.model,
            response.data.usage,
            response.metadata.request_id(),
            decide(&response.data)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_judgments_require_known_confident_answers_before_escalation() {
        for (answers, expected) in [
            (
                json!({"urgent": {"type": "noul", "noul": 0.95}, "team": {"type": "choice", "choice": "billing", "confidence": 0.9, "probabilities": {}}}),
                Decision::EscalateBilling,
            ),
            (
                json!({"urgent": {"type": "noul", "noul": 0.95}, "team": {"type": "choice", "choice": "technical", "confidence": 0.9, "probabilities": {}}}),
                Decision::EscalateTechnical,
            ),
            (json!({}), Decision::Review),
            (
                json!({"urgent": {"type": "noul", "noul": 0.1}}),
                Decision::Review,
            ),
            (
                json!({"urgent": {"type": "noul", "noul": 0.95}, "team": {"type": "noul", "noul": 0.9}}),
                Decision::Review,
            ),
            (
                json!({"urgent": {"type": "noul", "noul": 0.95}, "team": {"type": "choice", "choice": "billing", "confidence": 0.5, "probabilities": {}}}),
                Decision::Review,
            ),
            (
                json!({"urgent": {"type": "noul", "noul": 0.95}, "team": {"type": "choice", "choice": "unrecognized", "confidence": 0.9, "probabilities": {}}}),
                Decision::Review,
            ),
        ] {
            let response = serde_json::from_value(
                json!({"model": "fixture", "usage": {}, "answers": answers}),
            )
            .unwrap();
            assert_eq!(decide(&response), expected);
        }
    }
}
