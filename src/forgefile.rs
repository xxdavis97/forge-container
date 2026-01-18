use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Instruction {
    From{ image: String },
    Copy{ src: String, dest: String },
    Run{ command: String },
    Workdir{ path: String },
    Env{ key: String, value: String },
    Entrypoint { args: Vec<String> },
}

pub struct Forgefile {
    pub instructions: Vec<Instruction>,
    pub context_dir: PathBuf,  // Directory containing the Containerfile
}

impl Forgefile {
    pub fn parse(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let context_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut instructions = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#") {  // Use || not "or"
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() < 2 {
                continue;
            }
            
            // Need to unwrap the Result first, then check the Option
            if let Ok(Some(instruction)) = Self::parse_command_line(parts) {
                instructions.push(instruction);
            }
        }
        
        Ok(Self { 
            instructions, 
            context_dir 
        })
    }

    fn parse_command_line(parts: Vec<&str>) -> Result<Option<Instruction>, Box<dyn std::error::Error>> {
        let command = parts[0].to_uppercase();
        let args = parts[1];

        match command.as_str() {
            "FROM" => {
                // No curly braces around the struct! Just use the struct directly
                Ok(Some(Instruction::From { image: args.to_string() }))
            }
            "COPY" => {
                let copy_parts: Vec<&str> = args.split_whitespace().collect();
                if copy_parts.len() < 2 {
                    return Err("COPY requires source and destination".into());
                }
                Ok(Some(Instruction::Copy { 
                    src: copy_parts[0].to_string(), 
                    dest: copy_parts[1].to_string()
                }))
            }
            "RUN" => {
                Ok(Some(Instruction::Run { command: args.to_string() }))
            }
            "WORKDIR" => {
                Ok(Some(Instruction::Workdir { path: args.to_string() }))
            }
            "ENV" => {
                let env_parts: Vec<&str> = args.splitn(2, '=').collect();
                if env_parts.len() < 2 {
                    return Err("ENV requires KEY=VALUE format".into());
                }
                Ok(Some(Instruction::Env { 
                    key: env_parts[0].to_string(), 
                    value: env_parts[1].to_string() 
                }))
            }
            "ENTRYPOINT" => {
                let args = parse_json_array(args)?;
                Ok(Some(Instruction::Entrypoint { args }))
            }
            _ => Ok(None),
        }
    }
}

fn parse_json_array(s: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err("ENTRYPOINT requires JSON array format: [\"cmd\", \"arg\"]".into());
    }
    
    let inner = &s[1..s.len()-1];
    let parts: Vec<String> = inner
        .split(',')
        .map(|p| p.trim().trim_matches('"').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    
    Ok(parts)
}