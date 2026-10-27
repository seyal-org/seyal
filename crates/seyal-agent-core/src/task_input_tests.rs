use crate::{TaskInput, TaskInputError, PROVISIONAL_MAX_TASK_INPUT_BYTES};

#[test]
fn task_input_rejects_empty_and_whitespace_only_text() {
    assert_eq!(TaskInput::new(""), Err(TaskInputError::Empty));
    assert_eq!(TaskInput::new(" \n\t "), Err(TaskInputError::Empty));
}

#[test]
fn task_input_counts_utf8_bytes_and_preserves_unicode() {
    let text = "e\u{301} 🧪";
    let input = TaskInput::new(text).expect("non-empty task input");

    assert_eq!(input.as_str(), text);
    assert_eq!(input.len_bytes(), text.len());
    assert!(input.len_bytes() > text.chars().count());
}

#[test]
fn task_input_accepts_the_provisional_limit_and_rejects_one_byte_over() {
    let at_limit = "x".repeat(PROVISIONAL_MAX_TASK_INPUT_BYTES);
    let over_limit = format!("{at_limit}x");

    assert!(TaskInput::new(&at_limit).is_ok());
    assert_eq!(
        TaskInput::new(&over_limit),
        Err(TaskInputError::TooLarge {
            actual_bytes: PROVISIONAL_MAX_TASK_INPUT_BYTES + 1,
            max_bytes: PROVISIONAL_MAX_TASK_INPUT_BYTES,
        })
    );
}

#[test]
fn task_input_debug_does_not_disclose_user_text() {
    let secret = "private task text that must not enter logs";
    let rendered = format!("{:?}", TaskInput::new(secret).unwrap());

    assert!(!rendered.contains(secret));
    assert!(rendered.contains("REDACTED"));
}

#[test]
fn task_input_size_error_does_not_disclose_user_text() {
    let secret = "sensitive".repeat(PROVISIONAL_MAX_TASK_INPUT_BYTES / 9 + 1);
    let error = TaskInput::new(&secret).unwrap_err();
    let rendered = format!("{error}; {error:?}");

    assert!(matches!(error, TaskInputError::TooLarge { .. }));
    assert!(!rendered.contains(&secret));
    assert!(rendered.contains("UTF-8 bytes"));
}
