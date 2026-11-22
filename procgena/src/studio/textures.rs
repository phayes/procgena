//! Telling the user when texture images fail to load, for example because
//! they couldn't be downloaded.

use bevy::asset::AssetLoadFailedEvent;
use bevy::prelude::*;

use crate::ui::toast::{ShowToast, ToastKind};

/// Most failed textures named in one message.
const NAMED: usize = 3;

/// Shows one error message for the textures that failed to load this frame.
pub fn report_texture_failures(
    mut failed: MessageReader<AssetLoadFailedEvent<Image>>,
    mut commands: Commands,
) {
    let failures: Vec<(String, String)> = failed
        .read()
        .map(|event| (event.path.to_string(), event.error.to_string()))
        .collect();
    if let Some(message) = failure_message(&failures) {
        commands.trigger(ShowToast {
            kind: ToastKind::Error,
            message,
        });
    }
}

/// The message for textures that failed to load, given as (path, reason)
/// pairs, or `None` if there are none. Names the first few.
fn failure_message(failures: &[(String, String)]) -> Option<String> {
    let ((first_path, first_error), _) = failures.split_first()?;
    let mut lines = vec![
        "Some textures could not be loaded, so what uses them may not appear. \
         Check the internet connection, then restart the studio."
            .to_owned(),
    ];
    lines.push(format!("{first_path}: {first_error}"));
    lines.extend(
        failures[1..]
            .iter()
            .take(NAMED - 1)
            .map(|(path, _)| path.clone()),
    );
    if failures.len() > NAMED {
        lines.push(format!("and {} more", failures.len() - NAMED));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(count: usize) -> Vec<(String, String)> {
        (0..count)
            .map(|i| (format!("textures/{i}.ktx2"), "http status: 404".to_owned()))
            .collect()
    }

    #[test]
    fn no_failures_need_no_message() {
        assert_eq!(failure_message(&[]), None);
    }

    #[test]
    fn the_message_names_the_first_few_failures() {
        let message = failure_message(&failures(5)).unwrap();
        let lines: Vec<&str> = message.lines().collect();
        assert_eq!(lines[1], "textures/0.ktx2: http status: 404");
        assert_eq!(
            &lines[2..],
            ["textures/1.ktx2", "textures/2.ktx2", "and 2 more"]
        );

        let one = failure_message(&failures(1)).unwrap();
        assert_eq!(one.lines().count(), 2);
    }
}
