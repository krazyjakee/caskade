use super::*;

#[test]
fn a_pairing_that_fails_says_why_in_caskades_words() {
    let (outcome, message) = pairing_outcome(&Ok(Paired::WrongCode), "Den");
    assert_eq!(outcome, PairingOutcome::WrongCode);
    assert_eq!(
        message.as_deref(),
        Some("The code entered on Den didn't match. Try again with a new code.")
    );
    assert_eq!(
        pairing_outcome(&Ok(Paired::Declined), "Den").1.as_deref(),
        Some("Den declined.")
    );
    assert_eq!(
        pairing_outcome(&Ok(Paired::Expired), "Den").0,
        PairingOutcome::Expired
    );
    let paired = Paired::Paired {
        server_cert_pem: String::new(),
    };
    assert_eq!(
        pairing_outcome(&Ok(paired), "Den"),
        (PairingOutcome::Paired, None)
    );
    for (_, message) in [
        pairing_outcome(&Ok(Paired::WrongCode), "Den"),
        pairing_outcome(&Ok(Paired::Expired), "Den"),
    ] {
        let message = message.unwrap().to_lowercase();
        assert!(!message.contains("moonlight") && !message.contains("sunshine"));
    }
}
