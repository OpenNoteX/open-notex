//! Reading a line typed into the checklist's add row.
//!
//! One line is the whole input: a title and, when it carries `#name`, the
//! project it is filed under. This is pure text, so it can be tested without a
//! database or a window.

/// A task the add row would create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInput {
    pub title: String,
    /// The name typed after `#`. A project that does not exist yet is created
    /// in the same transaction as the task.
    pub project: Option<String>,
}

/// Parse `Send invites #Launch`. `None` means there was no title to make a task
/// from, so pressing Return does nothing rather than adding an empty row.
///
/// The first `#name` token is the project. A later `#` is part of the title, so
/// a task may still begin with a `#`.
pub fn parse_task_input(input: &str) -> Option<TaskInput> {
    let mut title: Vec<&str> = Vec::new();
    let mut project = None;
    for token in input.split_whitespace() {
        if project.is_none()
            && let Some(name) = token.strip_prefix('#')
            && !name.is_empty()
        {
            project = Some(name.to_owned());
            continue;
        }
        title.push(token);
    }
    let title = title.join(" ");
    if title.is_empty() {
        return None;
    }
    Some(TaskInput { title, project })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_line_is_just_a_title() {
        assert_eq!(
            parse_task_input("  Send invites  "),
            Some(TaskInput {
                title: "Send invites".to_owned(),
                project: None,
            })
        );
    }

    #[test]
    fn a_hash_token_files_the_task_into_a_project() {
        assert_eq!(
            parse_task_input("Send invites #Launch"),
            Some(TaskInput {
                title: "Send invites".to_owned(),
                project: Some("Launch".to_owned()),
            })
        );
        assert_eq!(
            parse_task_input("#Launch send invites"),
            Some(TaskInput {
                title: "send invites".to_owned(),
                project: Some("Launch".to_owned()),
            })
        );
    }

    #[test]
    fn an_empty_hash_is_part_of_the_title() {
        assert_eq!(
            parse_task_input("Write # 1"),
            Some(TaskInput {
                title: "Write # 1".to_owned(),
                project: None,
            })
        );
    }

    #[test]
    fn only_the_first_hash_is_a_project() {
        assert_eq!(
            parse_task_input("Ship #Launch #1"),
            Some(TaskInput {
                title: "Ship #1".to_owned(),
                project: Some("Launch".to_owned()),
            })
        );
    }

    #[test]
    fn a_line_with_no_title_is_not_a_task() {
        assert_eq!(parse_task_input(""), None);
        assert_eq!(parse_task_input("   "), None);
        assert_eq!(parse_task_input("#Launch"), None);
    }
}
