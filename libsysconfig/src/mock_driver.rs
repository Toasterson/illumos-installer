use crate::{CommandOutput, Instruction};

pub fn apply_instruction(root_path: &str, instruction: Instruction) -> anyhow::Result<CommandOutput> {
    Ok(CommandOutput {
        command: String::from("mock"),
        root_path: root_path.clone().into(),
        output: format!("instruction: {:?}", instruction),
    })
}