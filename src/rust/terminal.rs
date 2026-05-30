#![allow(clippy::unit_arg)]
//! terminal web socket CGI
#[macro_export]
macro_rules! send {
    ($($arg:tt)*) => (
        //use std::io::Write;

        let s = format!($($arg)* ) ;
        /*let l = s.len();
        println!("{l}");*/
        match write!(stdout(), "{s}") {
            Ok(_) => stdout().flush().unwrap(),
            Err(x) => panic!("Unable to write to stdout (file handle closed?): {}", x),
        }
    )
}

extern crate simcolor;
extern crate simtime;
extern crate simweb;

#[cfg(target_os = "windows")]
use std::os::windows::prelude::*;
use std::{
    collections::HashMap,
    env,
    error::Error,
    fmt,
    fs::{self, Metadata, OpenOptions},
    io::{self, BufRead, BufReader, ErrorKind, Read, Stdin, Write, stdout},
    ops::Not,
    path::{Component, MAIN_SEPARATOR_STR, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::UNIX_EPOCH,
};

use simcolor::Colorized;

pub const VERSION: &str = env!("VERSION");

const TERMINAL_NAME: &str = "sim/terminal";

const MAX_BLOCK_LEN: usize = 4096;

const DIR_COLOR: u8 = 41;

pub trait Terminal {
    /// returns values for terminal session as
    /// - current working directory
    /// - home directory
    /// - command aliases
    /// - version
    fn init(&self) -> (PathBuf, PathBuf, HashMap<String, Vec<String>>, &str);
    fn save_state(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn persist_cwd(&mut self, _cwd: &Path) {}
    fn greeting(&self, version: &str) -> String {
        let ver = version.color_num(66).to_string();
        format!("OS terminal {ver}")
    }
    fn main_loop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        term_loop(self)
    }
}

pub trait IsExecutable {
    /// Returns `true` if there is a file at the given path and it is
    /// executable. Returns `false` otherwise.
    ///
    /// See the module documentation for details.
    fn is_executable(&self) -> bool;
}
#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::IsExecutable;

    impl IsExecutable for Path {
        fn is_executable(&self) -> bool {
            let metadata = match self.metadata() {
                Ok(metadata) => metadata,
                Err(_) => return false,
            };
            let permissions = metadata.permissions();
            metadata.is_dir() || metadata.is_file() && permissions.mode() & 0o111 != 0
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::path::Path;

    use super::IsExecutable;
    impl IsExecutable for Path {
        fn is_executable(&self) -> bool {
            let Ok(metadata) = self.metadata() else {
                return false;
            };
            metadata.is_dir() || self.extension().is_some_and(|s| s == "exe" || s == "bat")
        }
    }
}
fn term_loop(term: &mut (impl Terminal + ?Sized)) -> Result<(), Box<dyn Error>> {
    let (mut cwd, def_dir, mut aliases, ver) = term.init();
    let mut stdin = io::stdin();

    send!("\n{}\n", &term.greeting(ver)); // {ver:?} {project} {session}");

    let mut child_env: HashMap<String, String> = env::vars()
        .filter(|(k, _)| {
            k != "GATEWAY_INTERFACE"
                && k != "QUERY_STRING"
                && k != "REMOTE_ADDR"
                && k != "REMOTE_HOST"
                && k != "REQUEST_METHOD"
                && k != "SERVER_PROTOCOL"
                && k != "SERVER_SOFTWARE"
                && k != "PATH_INFO"
                && k != "PATH_TRANSLATED"
                && k != "SCRIPT_NAME"
                && k != "REMOTE_IDENT"
                && k != "SERVER_NAME"
                && k != "SERVER_PORT"
                && k != "CONTENT_LENGTH"
                && k != "CONTENT_TYPE"
                && k != "AUTH_TYPE"
                && k != "REMOTE_USER"
                && !k.starts_with("HTTP_")
                && k != "_"
                && k != "PWD"
        })
        .collect();
    send!("{}\u{000C}", cwd.to_string_lossy().color_num(DIR_COLOR));
    // a decision about supporting colors can be done in init()
    let mut buffer = [0_u8; MAX_BLOCK_LEN];
    let mut prev: Option<Vec<u8>> = None;
    loop {
        let vec_buf = match prev {
            None => {
                let Ok(len) = stdin.read(&mut buffer) else {
                    break;
                };
                if len == 0 {
                    break;
                };
                &buffer[0..len]
            }
            Some(ref vec) => vec,
        };
        if vec_buf.len() >= 4
            && vec_buf[0] == 255
            && vec_buf[1] == 255
            && vec_buf[2] == 255
            && vec_buf[3] == 4
        {
            break;
        }
        if vec_buf.len() == 1 && vec_buf[0] == 3 {
            send!("^C\n");
            continue;
        }
        let line = String::from_utf8_lossy(vec_buf).into_owned();
        prev = None;
        let expand = line.ends_with('\t');
        let (mut cmd, piped, in_file, out_file, appnd, bkgr) =
            parse_cmd(&line.trim(), &child_env, &cwd);
        if cmd.is_empty() {
            continue;
        };
        //eprintln!("pipe {piped:?} - {in_file} < {cmd:?} > {out_file}");
        if expand {
            let ext = esc_string_blanks(extend_name(
                if out_file.is_empty() {
                    if in_file.is_empty() {
                        &cmd[cmd.len() - 1]
                    } else {
                        &in_file
                    }
                } else {
                    &out_file
                },
                &cwd,
                cmd.len() == 1,
            ));
            let mut beg = piped.into_iter().fold(String::new(), |a, e| {
                a + &e
                    .into_iter()
                    .reduce(|a2, e2| a2 + " " + &esc_string_blanks(e2))
                    .unwrap()
                    + "|"
            });

            if cmd.len() > 1 {
                if out_file.is_empty() && in_file.is_empty() {
                    cmd.pop();
                }
                beg += &cmd
                    .into_iter()
                    .reduce(|a, e| a + " " + &esc_string_blanks(e))
                    .unwrap();
                if !out_file.is_empty() {
                    if !in_file.is_empty() {
                        beg.push('<');
                        beg.push_str(&in_file)
                    }
                    if appnd {
                        beg.push('>')
                    }
                    beg.push('>');
                } else if !in_file.is_empty() {
                    beg.push('<')
                }
            }
            //eprintln!("line to send {} {ext}", beg);
            send!("\r{} {ext}", beg); // &line[..pos]);
            continue;
        }
        send!("{line}"); // \n is coming as part of command
        if piped.is_empty() {
            // think on condition to do that more
            cmd = expand_alias(&aliases, cmd, &child_env);
        }
        match cmd[0].as_str() {
            "dir" if cfg!(windows) && out_file.is_empty() => {
                let names_only = cmd.len() > 1 && cmd[1] == "/b";
                let mut dir = if cmd.len() == if names_only { 2 } else { 1 } {
                    cwd.clone()
                } else {
                    PathBuf::from(&cmd[if names_only { 2 } else { 1 }])
                };
                let file_details = |metadata: &Metadata, res: &mut String| {
                    let tz = (simtime::get_local_timezone_offset_dst().0 * 60) as i64;
                    let (y, m, d, h, mm, _s, _) = simtime::get_datetime(
                        1970,
                        (metadata
                            .modified()
                            .unwrap()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64
                            + tz) as u64,
                    );
                    let ro = metadata.permissions().readonly();
                    let link = metadata.is_symlink();
                    #[cfg(target_os = "windows")]
                    const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x00000020;
                    #[cfg(target_os = "windows")]
                    let archive = (metadata.file_attributes() & FILE_ATTRIBUTE_ARCHIVE) > 0;
                    #[cfg(unix)]
                    let archive = false;
                    if metadata.is_dir() {
                        res.push('d')
                    } else {
                        res.push('-')
                    }
                    res.push(if archive { 'a' } else { '-' });
                    if ro {
                        res.push('r')
                    } else {
                        res.push('-')
                    }
                    #[cfg(target_os = "windows")]
                    {
                        let attributes = metadata.file_attributes();
                        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x00000002;
                        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x00000004;

                        if (attributes & FILE_ATTRIBUTE_HIDDEN) > 0 {
                            // Check if the hidden attribute is set.
                            res.push('h')
                        } else {
                            res.push('-')
                        }
                        if (attributes & FILE_ATTRIBUTE_SYSTEM) > 0 {
                            // Check if the system attribute is set.
                            res.push('s')
                        } else {
                            res.push('-')
                        }
                    }
                    if link {
                        res.push('l')
                    } else {
                        res.push('-')
                    }
                    let (h, pm) = match h {
                        0 => (12, 'A'),
                        h @ 1..12 => (h, 'A'),
                        12 => (12, 'P'),
                        h @ 13..24 => (h - 12, 'P'),
                        _ => unreachable!(),
                    };
                    let date = &format!("{m:>2}/{d}/{y:4}");
                    res.push_str(&format!(
                        "{:8}{date:>10}  {h:>2}:{mm:02} {}M {:>14} ",
                        ' ',
                        pm,
                        EntryLen(metadata)
                    ));
                };
                if !dir.has_root() {
                    dir = cwd.join(dir);
                }
                let mut res: String;
                if !names_only {
                    res = format!("    Directory: {}\n\n", dir.display());
                    res.push_str("Mode                 LastWriteTime         Length Name\n");
                    res.push_str("----                 -------------         ------ ----\n");
                } else {
                    res = String::new();
                }
                if dir.display().to_string().find('*').is_none() {
                    let Ok(paths) = fs::read_dir(&dir) else {
                        send!("{} is invalid\u{000C}", dir.to_string_lossy().red());
                        continue;
                    };
                    for path in paths {
                        let Ok(path) = path else { continue };
                        if !names_only {
                            let metadata = path.metadata()?;
                            file_details(&metadata, &mut res);
                        }
                        let path = path.path();
                        let mut file_name = if let Some(name) = path.file_name() {
                            name.display().to_string().default()
                        } else {
                            "???".to_string().red()
                        };
                        if path.is_symlink() {
                            file_name = file_name.cyan()
                        } else if path.is_dir() {
                            file_name = file_name.color_num(27)
                        } else if let Some(ext) = path.extension() {
                            let ext = ext.to_str().unwrap();
                            match ext {
                                "exe" | "com" | "bat" | "msi" => {
                                    file_name = file_name.bright().green()
                                }
                                "zip" | "gz" | "rar" | "7z" | "xz" | "jar" | "tgz" | "bz2"
                                | "war" | "tar" => file_name = file_name.red(),
                                "jpeg" | "jpg" | "png" | "bmp" | "gif" => {
                                    file_name = file_name.magenta()
                                }
                                "txt" | "md" => file_name = file_name.yellow(),
                                "7b" | "rb" => file_name = file_name.color_num(183),
                                _ => (),
                            }
                        }
                        res.push_str(&format!("{file_name}\n"));
                    }
                    send!("{res}\u{000C}");
                } else {
                    let data = DeferData::from(&cwd, &dir);
                    for arg in data.src_wild {
                        dir.pop();
                        dir.push(format! {"{}{arg}{}",&data.src_before, &data.src_after});
                        if !names_only {
                            file_details(&dir.metadata()?, &mut res);
                        }
                        if let Some(file_name) = dir.file_name() {
                            res.push_str(&file_name.display().to_string());
                            res.push('\n');
                        }
                    }
                    send!("{res}\u{000C}");
                }
            }
            "pwd" => {
                send!("{}\u{000C}", cwd.to_string_lossy().color_num(DIR_COLOR));
            }
            "cd" => {
                let mut cwd_new;
                if cmd.len() == 1 {
                    cwd_new = def_dir.clone();
                } else {
                    cwd_new = PathBuf::from(&cmd[1]);
                    if !cwd_new.has_root() {
                        cwd_new = cwd.clone();
                        cwd_new.push(&cmd[1])
                    }
                }
                cwd_new = remove_redundant_components(&cwd_new);
                if cwd_new.is_dir() {
                    cwd = cwd_new;
                    term.persist_cwd(&cwd);
                    child_env.insert("PWD".to_string(), cwd.display().to_string());
                    send!("{}\u{000C}", cwd.to_string_lossy().color_num(DIR_COLOR));
                } else {
                    if cfg!(windows) {
                        send!("The system cannot find the path specified.\u{000C}");
                    } else {
                        send!(
                            "cd: no such directory: {}\u{000C}",
                            cwd_new.to_string_lossy().color_num(161)
                        );
                    }
                }
            }
            "del" if cfg!(windows) => {
                if cmd.len() == 1 {
                    send!("No name specified\u{000C}");
                    continue;
                }
                send!(
                    "{} file(s) deleted\u{000C}",
                    DeferData::from(&cwd, &PathBuf::from(&cmd[1]))
                        .do_op(Op::DEL)
                        .unwrap()
                );
            }
            "type" if cfg!(windows) && out_file.is_empty() => {
                if cmd.len() == 1 {
                    send!("The syntax of the command is incorrect.\u{000C}");
                    continue;
                }
                // wild card Windows specific
                if let Err(cause) = DeferData::from(&cwd, &PathBuf::from(&cmd[1])).do_op(Op::TYP) {
                    send!("Can't show the file : {cause}");
                }
                send!("\u{000C}");
            }
            "copy" | "ren" if cfg!(windows) => {
                if cmd.len() != 3 {
                    send!("Only one source and destination have to be provided\u{000C}");
                    continue;
                }
                let file = PathBuf::from(&cmd[1]);
                let file_to = PathBuf::from(&cmd[2]);
                match cmd[0].as_str() {
                    "copy" => {
                        send!(
                            "{} file(s) copied\u{000C}",
                            DeferData::from_to(&cwd, &file, &file_to)
                                .do_op(Op::CPY)
                                .unwrap()
                        );
                    }
                    "ren" => {
                        send!(
                            "{} file(s) renamed\u{000C}",
                            DeferData::from_to(&cwd, &file, &file_to)
                                .do_op(Op::REN)
                                .unwrap()
                        );
                    }
                    _ => unreachable!(),
                }
            }
            "echo" if cfg!(windows) && out_file.is_empty() => {
                if cmd.len() == 2 {
                    if !out_file.is_empty()
                    /*None*/
                    {
                        let mut file = PathBuf::from(&out_file);
                        if !file.has_root() {
                            file = cwd.join(file);
                        }
                        fs::write(file, &cmd[1])?;
                        send!("\u{000C}");
                    } else {
                        send!("{}\u{000C}", cmd[1]);
                    }
                }
            }
            "md" | "mkdir" if cfg!(windows) => {
                if cmd.len() == 1 {
                    send!("No name specified\u{000C}");
                    continue;
                }
                let mut file = PathBuf::from(&cmd[1]);
                if !file.has_root() {
                    file = cwd.join(file);
                }
                match fs::create_dir(file) {
                    Ok(_) => {
                        send!("{} created\u{000C}", cmd[1]);
                    }
                    Err(err) => {
                        send!("Err: {err} in {} creation\u{000C}", cmd[1]);
                    }
                }
            }
            "rmdir" if cfg!(windows) => {
                if cmd.len() == 1 {
                    send!("No name specified\u{000C}");
                    continue;
                }
                let mut file = PathBuf::from(&cmd[1]);
                if !file.has_root() {
                    file = cwd.join(file);
                }
                match fs::remove_dir_all(file) {
                    Ok(_) => {
                        send!("{} removed\u{000C}", cmd[1]);
                    }
                    Err(err) => {
                        send!("Err: {err} in removing {}\u{000C}", cmd[1]);
                    }
                }
            }
            "export" => {
                if cmd.len() != 2 {
                    send!("A parameter in the form - name=value has to be specified\u{000C}");
                    continue;
                }
                if let Some((name, value)) = cmd[1].split_once('=') {
                    child_env.insert(name.to_string(), value.to_string());
                    send!("\u{000C}");
                } else {
                    send!("The parameter has to be in the name=value form\u{000C}");
                }
                continue;
            }
            "unset" => {
                if cmd.len() != 2 {
                    send!("Name of an environment variable is not specified\u{000C}");
                    continue;
                }
                child_env.remove(&cmd[1]);
                send!("\u{000C}");
            }
            "set" if cfg!(windows) => {
                match cmd.len() {
                    1 => {
                        for (key, value) in &child_env {
                            send!("{}={}\n", key, value);
                        }
                    }
                    2 => {
                        if let Some((name, value)) = cmd[1].split_once('=') {
                            if value.is_empty() {
                                child_env.remove(name);
                            } else {
                                child_env.insert(name.to_string(), value.to_string());
                            }
                        } else {
                            send!("The parameter has to be in a form: name=value");
                        }
                    }
                    _ => {
                        send!("Invalid number of parameters");
                    }
                }
                send!("\u{000C}");
            }
            "alias" => {
                if cmd.len() == 1 {
                    for (alias, extension) in &aliases {
                        send!("alias {alias}='{}'\n", extension.join(" "));
                    }
                } else if cmd.len() == 2
                    && let Some((name, value)) = cmd.get(1).unwrap().split_once('=')
                {
                    let name = name.trim();
                    let value = value.trim_matches(['"', '\'', ' ']);
                    aliases.insert(
                        name.to_string(),
                        value.split_ascii_whitespace().map(str::to_string).collect(),
                    );
                } else {
                    send!("Invalid number of arguments of alias");
                }
                send!("\u{000C}");
            }
            "ver!" => {
                send!("{VERSION}\u{000C}"); // path
            }
            _ => {
                child_env.insert("_".to_string(), cmd[0].clone());
                if piped.is_empty() {
                    if in_file.is_empty() && out_file.is_empty() {
                        if bkgr {
                            if let Ok(pid) = call_process_async(&cmd, &cwd, &child_env) {
                                send!("[{}] {pid}\u{000C}", cmd[0]);
                            }
                        } else {
                            prev = call_process(cmd, &cwd, &stdin, &child_env);
                        }
                    } else if in_file.is_empty() {
                        if !out_file.is_empty()
                        /*None*/
                        {
                            let mut file = PathBuf::from(&out_file);
                            if !file.has_root() {
                                file = cwd.join(file);
                            }
                            let mut file = OpenOptions::new()
                                .write(true)
                                .append(appnd)
                                .create(!appnd)
                                .truncate(!appnd)
                                .open(file)?;
                            prev = call_process_out_file(cmd, &cwd, &stdin, &mut file, &child_env);
                        } else {
                            prev = call_process(cmd, &cwd, &stdin, &child_env);
                        }
                    } else {
                        let mut in_file = PathBuf::from(in_file);
                        if !in_file.has_root() {
                            in_file = cwd.join(in_file);
                        }
                        if let Ok(contents) = fs::read(&in_file) {
                            match call_process_piped(&cmd, &cwd, &contents, &child_env) {
                                Ok(res) => {
                                    if out_file.is_empty() {
                                        send!("{}\u{000C}", String::from_utf8_lossy(&res));
                                    } else {
                                        let mut out_file = PathBuf::from(out_file);
                                        if !out_file.has_root() {
                                            out_file = cwd.join(out_file);
                                        }
                                        let _ = fs::write(&out_file, res);
                                        send!("\u{000C}");
                                    }
                                }
                                Err(err) => {
                                    send!(
                                        "Command {} not found in the pipe, reason: {err}\u{000C}",
                                        &cmd[0].clone().red()
                                    );
                                }
                            }
                        } else {
                            send!("Can't read {}\u{000C}", in_file.display().to_string().red());
                        }
                    }
                } else {
                    // piping work
                    let mut res = vec![];
                    for mut pipe_cmd in piped {
                        pipe_cmd = expand_alias(&aliases, pipe_cmd, &child_env);
                        match call_process_piped(&pipe_cmd, &cwd, &res, &child_env) {
                            Ok(next_res) => {
                                res = next_res;
                            }
                            Err(err) => {
                                send!(
                                    "Command {} not found in the pipe, reason: {err}\u{000C}",
                                    &pipe_cmd[0].clone().red()
                                );
                                break;
                            }
                        }
                        //eprintln!("Called {pipe_cmd:?} returned {}", String::from_utf8_lossy(&res));
                    }
                    cmd = expand_alias(&aliases, cmd, &child_env);
                    //eprintln!("before call {cmd:?}");
                    match call_process_piped(&cmd, &cwd, &res, &child_env) {
                        Ok(res) => {
                            if out_file.is_empty() {
                                send!("{}\u{000C}", String::from_utf8_lossy(&res));
                            } else {
                                let mut out_file = PathBuf::from(out_file);
                                if !out_file.has_root() {
                                    out_file = cwd.join(out_file);
                                }
                                let _ = fs::write(&out_file, res);
                                send!("\u{000C}");
                            }
                        }
                        Err(err) => {
                            send!(
                                "Command {} not found in the pipe, reason: {err}\u{000C}",
                                &cmd[0].clone().red()
                            );
                        }
                    }
                }
            }
        }
    }

    term.save_state()
}

fn call_process(
    cmd: Vec<String>,
    cwd: &PathBuf,
    mut stdin: &Stdin,
    filtered_env: &HashMap<String, String>,
) -> Option<Vec<u8>> {
    let mut binding = Command::new(adjust_cmd(cwd, cmd[0].clone()));
    let mut process = binding
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(filtered_env)
        .current_dir(cwd);
    if cmd.len() > 1 {
        process = process.args(&cmd[1..])
    }
    let mut res: Option<Vec<u8>> = None;
    match process.spawn() {
        Ok(mut process) => {
            // TODO consider
            // let (mut recv, send) = std::io::pipe()?;
            let mut stdout = process.stdout.take()?;
            let mut stdin_child = process.stdin.take()?;
            let stderr = process.stderr.take()?;
            let share_process = Arc::new(Mutex::new(process));
            let for_kill = Arc::clone(&share_process);
            let for_wait = Arc::clone(&share_process);
            thread::scope(|s| {
                let err_col = s.spawn(|| {
                    let reader = BufReader::new(stderr);
                    /* it waits for new output */
                    for line in reader.lines() {
                        let string = line.unwrap();
                        send! {"{}\n", string};
                    }
                });

                s.spawn(|| {
                    let mut buffer = [0_u8; MAX_BLOCK_LEN];
                    while let Ok(mut len) = stdin.read(&mut buffer)
                        && len > 0
                    {
                        let mut start = 0;
                        if len == 1 && buffer[0] == 3 && for_kill.lock().unwrap().kill().is_ok() {
                            send!("^C");
                            break;
                        } else if len == 2 && buffer[0] == b'.' && buffer[1] == b'\n' {
                            drop(stdin_child);
                            break;
                        } else if len == 3
                            && buffer[0] == b'.'
                            && buffer[1] == b'.'
                            && buffer[2] == b'\n'
                        {
                            start = 1;
                        }
                        let eof = if buffer[len - 1] == 0x1a {
                            // EOF
                            len -= 1;
                            if start == len {
                                drop(stdin_child);
                                #[cfg(target_os = "windows")]
                                send!("^D");
                                #[cfg(not(target_os = "windows"))]
                                send!("^Z");
                                break;
                            }
                            true
                        } else {
                            false
                        };
                        //let line = String::from_utf8_lossy(&buffer[0..len]);
                        match stdin_child.write_all(&buffer[start..len]) {
                            Ok(()) => {
                                stdin_child.flush().unwrap(); // can be an error?
                                send! {"{}", String::from_utf8_lossy(&buffer[0..len])} // echo
                                res = None; // user input consumed by the child process
                                if eof {
                                    drop(stdin_child);
                                    #[cfg(target_os = "windows")]
                                    send!("^D");
                                    #[cfg(not(target_os = "windows"))]
                                    send!("^Z");
                                    break;
                                }
                            }
                            Err(_) => {
                                res = Some(buffer[0..len].to_vec()); // user input goes in the terminal way
                                break;
                            }
                        }
                    }
                });

                //s.spawn(|| {
                let mut buffer = [0_u8; MAX_BLOCK_LEN];
                while let Ok(l) = stdout.read(&mut buffer)
                    && l > 0
                {
                    let data = buffer[..l].to_vec();
                    let string = String::from_utf8_lossy(&data);
                    send! {"{}", string};
                }
                //});

                for_wait.lock().unwrap().wait().unwrap();
                let _ = err_col.join();
                send!("\u{000C}");
            });
        }
        Err(err) => {
            send!(
                "Command {} not found in {cwd:?}, reason: {err}\u{000C}",
                cmd[0].clone().red().bold()
            );
        }
    }
    res
}

fn call_process_out_file(
    cmd: Vec<String>,
    cwd: &PathBuf,
    mut stdin: &Stdin,
    out: &mut dyn Write,
    filtered_env: &HashMap<String, String>,
) -> Option<Vec<u8>> {
    if cfg!(windows) {
        match emulate_unix_cmd(&cmd, cwd) {
            Ok(Some(vec)) => {
                if let Err(err) = out.write_all(&vec) {
                    send!(
                        "Error: {err} at writing in {cwd:?} of {}",
                        cmd[0].clone().red()
                    );
                }
                send!("\u{000C}");
                return None;
            }
            Err(err) => {
                send!(
                    "Command {} not found in {cwd:?}, reason: {err}\u{000C}",
                    cmd[0].clone().red().bold()
                );
                return None;
            }
            Ok(None) => (),
        }
    }
    let mut binding = Command::new(adjust_cmd(cwd, cmd[0].clone()));
    let mut process = binding
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(filtered_env)
        .current_dir(cwd);
    if cmd.len() > 1 {
        process = process.args(&cmd[1..])
    }
    let process = process.spawn();
    let mut res: Option<Vec<u8>> = None;
    match process {
        Ok(mut process) => {
            let mut stdout = process.stdout.take()?;
            let mut stdin_child = process.stdin.take()?;
            let stderr = process.stderr.take()?;
            let share_process = Arc::new(Mutex::new(process));
            let for_kill = Arc::clone(&share_process);
            let for_wait = Arc::clone(&share_process);
            thread::scope(|s| {
                s.spawn(|| {
                    let reader = BufReader::new(stderr);
                    /* it waits for new output */
                    for line in reader.lines() {
                        let string = line.unwrap();
                        send! {"{}\n", string};
                    }
                });

                s.spawn(|| {
                    let mut buffer = [0_u8; MAX_BLOCK_LEN];
                    while let Ok(mut len) = stdin.read(&mut buffer)
                        && len > 0
                    {
                        //eprintln!("l:{}",simweb::to_hex(&buffer[..len]));
                        let mut start = 0;
                        if len == 1 && buffer[0] == 3 && for_kill.lock().unwrap().kill().is_ok() {
                            send!("^C");
                            break;
                        } else if len == 2 && buffer[0] == b'.' && buffer[1] == b'\n' {
                            drop(stdin_child);
                            break;
                        } else if len == 3
                            && buffer[0] == b'.'
                            && buffer[1] == b'.'
                            && buffer[2] == b'\n'
                        {
                            start = 1;
                        }
                        let eof = if buffer[len - 1] == 0x1a {
                            // EOF
                            len -= 1;
                            if start == len {
                                drop(stdin_child);
                                #[cfg(target_os = "windows")]
                                send!("^D");
                                #[cfg(not(target_os = "windows"))]
                                send!("^Z");
                                break;
                            }
                            true
                        } else {
                            false
                        };

                        match stdin_child.write_all(&buffer[start..len]) {
                            Ok(()) => {
                                stdin_child.flush().unwrap(); // can be an error?
                                send! {"{}", String::from_utf8_lossy(&buffer[0..len])} // echo
                                res = None; // user input consumed by the child process
                                if eof {
                                    drop(stdin_child);
                                    #[cfg(target_os = "windows")]
                                    send!("^D");
                                    #[cfg(not(target_os = "windows"))]
                                    send!("^Z");
                                    break;
                                }
                            }
                            Err(_) => {
                                res = Some(buffer[0..len].to_vec()); // user input goes in the terminal way
                                break;
                            }
                        }
                    }
                });

                let mut buffer = [0_u8; MAX_BLOCK_LEN];
                while let Ok(l) = stdout.read(&mut buffer)
                    && l > 0
                {
                    if let Err(err) = out.write(&buffer[..l]) {
                        send!(
                            "Error: {err} at writing in {cwd:?} of {}",
                            cmd[0].clone().red()
                        );
                        break;
                    }
                }

                for_wait.lock().unwrap().wait().unwrap();
                send!("\u{000C}");
            });
        }
        Err(err) => {
            send!(
                "Command {} not found in {cwd:?}, reason: {err}\u{000C}",
                cmd[0].clone().red().bold()
            );
        }
    }
    res
}

fn call_process_piped(
    cmd: &[String],
    cwd: &PathBuf,
    in_pipe: &[u8],
    filtered_env: &HashMap<String, String>,
) -> io::Result<Vec<u8>> {
    if cfg!(windows) {
        match emulate_unix_cmd(cmd, cwd) {
            Ok(Some(vec)) => return Ok(vec),
            Err(err) => return Err(err),
            Ok(None) => (),
        }
    }
    let mut binding = Command::new(adjust_cmd(cwd, cmd[0].clone()));
    let mut process = binding
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(filtered_env)
        .current_dir(cwd);
    if cmd.len() > 1 {
        process = process.args(&cmd[1..])
    }
    let mut process = process.spawn()?;
    let mut stdout = process.stdout.take().unwrap();
    let stderr = process.stderr.take().unwrap();
    let mut stdin_child = process.stdin.take().unwrap();
    let handle = thread::spawn(move || {
        let mut buffer = [0_u8; MAX_BLOCK_LEN];
        let mut res = vec![];
        while let Ok(l) = stdout.read(&mut buffer)
            && l > 0
        {
            res.extend_from_slice(&buffer[..l])
        }
        res
    });

    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let string = line.unwrap();
            send! {"{}\n", string};
        }
    });

    if stdin_child.write_all(in_pipe).is_ok() {
        stdin_child.flush().unwrap()
    }
    drop(stdin_child);
    process.wait().unwrap();
    Ok(handle.join().unwrap())
}

fn call_process_async(
    cmd: &[String],
    cwd: &PathBuf,
    filtered_env: &HashMap<String, String>,
) -> io::Result<u32> {
    let mut binding = Command::new(adjust_cmd(cwd, cmd[0].clone()));
    let mut command = binding
        .stdout(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env_clear()
        .envs(filtered_env)
        .current_dir(cwd);
    if cmd.len() > 1 {
        command = command.args(&cmd[1..])
    };
    Ok(command.spawn()?.id())
}

#[derive(Debug, Clone, PartialEq, Default)]
enum CmdState {
    #[default]
    StartArg,
    QuotedArg,
    DblQuotedArg,
    InArg,
    Esc,
    QEsc,
    DEsc,
}

#[derive(Debug, Clone, PartialEq, Default)]
enum RedirectSate {
    #[default]
    NoRedirect,
    Input,
    Output,
}

fn parse_cmd(
    input: &impl AsRef<str>,
    child_env: &HashMap<String, String>,
    cwd: &Path,
) -> (Vec<String>, Vec<Vec<String>>, String, String, bool, bool) {
    // TODO add < for first group and > for last group which can be be the same
    let mut pipe_res = vec![];
    let mut res = vec![];
    let mut input_file = String::new();
    let mut output_file = String::new();
    let mut asynch = false;
    let mut append = false;
    let mut state = Default::default();
    let mut curr_comp = String::new();
    let mut red_state = RedirectSate::default();
    let input = input.as_ref();
    let mut arg_segment = String::with_capacity(256);
    let mut was_blob = false;
    for c in input.chars() {
        match c {
            ' ' | '\t' | '\r' | '\n' | '\u{00a0}' | '|' | '(' | ')' | '<' | '>' | ';' | '&'
            | '\u{000C}' | '\u{000B}' => {
                // \f \v
                match state {
                    CmdState::StartArg => {
                        match c {
                            '|' => {
                                // finish the command + args group and start a new one
                                pipe_res.push(res.clone());
                                res.clear();
                            }
                            '<' => {
                                red_state = RedirectSate::Input;
                            }
                            '>' => match red_state {
                                RedirectSate::Output => append = true,
                                _ => red_state = RedirectSate::Output,
                            },
                            '&' => asynch = true,
                            _ => (),
                        }
                    }
                    CmdState::InArg => {
                        state = CmdState::StartArg;
                        if arg_segment.is_empty().not() {
                            curr_comp.push_str(&interpolate_env(&arg_segment, child_env))
                        }
                        match red_state {
                            RedirectSate::NoRedirect => {
                                if was_blob {
                                    expand_wildcard_in_arg(cwd, curr_comp.clone(), &mut res)
                                } else {
                                    res.push(curr_comp.clone());
                                }
                            }
                            RedirectSate::Input => {
                                input_file = String::from(&curr_comp);
                            }
                            RedirectSate::Output => {
                                output_file = String::from(&curr_comp);
                            }
                        }
                        arg_segment.clear();
                        curr_comp.clear();
                        match c {
                            '|' => {
                                pipe_res.push(res.clone());
                                res.clear();
                            }
                            '<' => {
                                red_state = RedirectSate::Input;
                            }
                            '>' => match red_state {
                                RedirectSate::Output => append = true,
                                _ => red_state = RedirectSate::Output,
                            },
                            '&' => asynch = true,
                            _ => red_state = RedirectSate::NoRedirect,
                        }
                    }
                    CmdState::Esc => {
                        state = CmdState::InArg;
                        arg_segment.push(c)
                    }
                    CmdState::QuotedArg => {
                        curr_comp.push(c);
                    }
                    CmdState::DblQuotedArg => {
                        arg_segment.push(c);
                    }
                    CmdState::QEsc => {
                        state = CmdState::QuotedArg;
                        curr_comp.push(c)
                    }
                    CmdState::DEsc => {
                        state = CmdState::DblQuotedArg;
                        arg_segment.push(c)
                    }
                }
            }
            '"' => {
                asynch = false;
                match state {
                    CmdState::StartArg => {
                        state = CmdState::DblQuotedArg;
                        was_blob = false;
                        //arg_segment.clear()
                    }
                    CmdState::InArg => {
                        state = CmdState::DblQuotedArg;
                        if arg_segment.is_empty().not() {
                            curr_comp.push_str(&interpolate_env(&arg_segment, child_env));
                            arg_segment.clear()
                        }
                    }
                    CmdState::QuotedArg => curr_comp.push(c),
                    CmdState::Esc => {
                        arg_segment.push('\\');
                        arg_segment.push(c);
                        state = CmdState::InArg;
                    }
                    CmdState::QEsc => {
                        curr_comp.push('\\');
                        curr_comp.push(c);
                        state = CmdState::QuotedArg;
                    }
                    CmdState::DEsc => {
                        state = CmdState::DblQuotedArg;
                        arg_segment.push(c)
                    }
                    CmdState::DblQuotedArg => {
                        curr_comp.push_str(&interpolate_env(&arg_segment, child_env));
                        arg_segment.clear(); // is it really required
                        state = CmdState::InArg;
                    }
                }
            }
            '\'' => {
                asynch = false;
                match state {
                    CmdState::StartArg => {
                        state = CmdState::QuotedArg;
                        was_blob = false;
                    }
                    CmdState::InArg => {
                        if arg_segment.is_empty().not() {
                            curr_comp.push_str(&interpolate_env(&arg_segment, child_env));
                            arg_segment.clear()
                        }
                        state = CmdState::QuotedArg;
                    }
                    CmdState::QuotedArg => state = CmdState::InArg,
                    CmdState::Esc => {
                        arg_segment.push('\\');
                        arg_segment.push(c);
                        state = CmdState::InArg;
                    }
                    CmdState::QEsc => {
                        curr_comp.push(c);
                        state = CmdState::QuotedArg;
                    }
                    CmdState::DEsc => {
                        arg_segment.push('\\');
                        arg_segment.push(c);
                        state = CmdState::DblQuotedArg;
                    }
                    CmdState::DblQuotedArg => arg_segment.push(c),
                }
            }
            '\\' => {
                asynch = false;
                match state {
                    CmdState::StartArg | CmdState::InArg => {
                        state = CmdState::Esc;
                    }
                    CmdState::QuotedArg => {
                        state = CmdState::QEsc;
                    }
                    CmdState::DblQuotedArg => {
                        state = CmdState::DEsc;
                    }
                    CmdState::Esc => {
                        state = CmdState::InArg;
                        arg_segment.push(c);
                    }
                    CmdState::QEsc => {
                        state = CmdState::QuotedArg;
                        curr_comp.push(c);
                    }
                    CmdState::DEsc => {
                        state = CmdState::DblQuotedArg;
                        arg_segment.push(c)
                    }
                }
            }
            '*' if !cfg!(windows) => {
                // Unix way not working for Windows
                asynch = false;
                match state {
                    CmdState::StartArg | CmdState::InArg => {
                        state = CmdState::InArg;
                        arg_segment.push(c);
                        was_blob = true;
                    }
                    CmdState::QuotedArg | CmdState::DblQuotedArg => {
                        curr_comp.push(c);
                    }
                    CmdState::Esc => {
                        state = CmdState::InArg;
                        arg_segment.push(c);
                    }
                    CmdState::QEsc => {
                        state = CmdState::QuotedArg;
                        curr_comp.push('\\');
                        curr_comp.push(c);
                    }
                    CmdState::DEsc => {
                        state = CmdState::DblQuotedArg;
                        arg_segment.push('\\');
                        arg_segment.push(c)
                    }
                }
            }
            other => {
                asynch = false;
                match state {
                    CmdState::StartArg | CmdState::InArg => {
                        state = CmdState::InArg;
                        arg_segment.push(other);
                    }
                    CmdState::QuotedArg => {
                        curr_comp.push(other);
                    }
                    CmdState::DblQuotedArg => arg_segment.push(other),
                    CmdState::Esc => {
                        state = CmdState::InArg;
                        arg_segment.push('\\');
                        arg_segment.push(c);
                    }
                    CmdState::QEsc => {
                        state = CmdState::QuotedArg;
                        curr_comp.push('\\');
                        curr_comp.push(c);
                    }
                    CmdState::DEsc => {
                        state = CmdState::DblQuotedArg;
                        arg_segment.push('\\');
                        arg_segment.push(c)
                    }
                }
            }
        }
    }

    if state == CmdState::Esc {
        arg_segment.push('\\');
        state = CmdState::InArg;
    } else if state == CmdState::DEsc {
        arg_segment.push('\\');
        state = CmdState::DblQuotedArg;
    } else if state == CmdState::QEsc {
        curr_comp.push('\\');
        state = CmdState::QuotedArg;
    }
    match state {
        CmdState::InArg | CmdState::QuotedArg | CmdState::DblQuotedArg => {
            if state == CmdState::DblQuotedArg || state == CmdState::InArg {
                curr_comp.push_str(&interpolate_env(&arg_segment, child_env))
            }
            match red_state {
                RedirectSate::NoRedirect => {
                    if was_blob {
                        expand_wildcard_in_arg(cwd, curr_comp, &mut res)
                    } else {
                        res.push(curr_comp);
                    }
                }
                RedirectSate::Input => {
                    input_file = String::from(&curr_comp);
                }
                RedirectSate::Output => {
                    output_file = String::from(&curr_comp);
                }
            }
        }
        CmdState::StartArg => (),
        _ => todo!(), // shouldn't happen ever
    }
    (res, pipe_res, input_file, output_file, append, asynch)
}

#[inline]
#[cfg(target_os = "windows")]
fn adjust_cmd(cwd: &Path, prog: String) -> String {
    if prog.starts_with(".\\") || prog.starts_with("..\\") {
        cwd.to_owned().join(prog).display().to_string()
    } else {
        prog
    }
}

#[inline]
#[cfg(not(target_os = "windows"))]
fn adjust_cmd(_cwd: &Path, prog: String) -> String {
    prog
}

fn expand_wildcard_in_arg(cwd: &Path, arg: String, args: &mut Vec<String>) {
    if arg.find('*').is_none() {
        args.push(arg);
    } else {
        let mut comp_path = PathBuf::from(&arg);
        let data = DeferData::from(cwd, &comp_path); // * is processed here
        if data.src_wild.is_empty() {
            args.push(arg)
        } else {
            comp_path.pop();
            for arg in data.src_wild {
                comp_path.push(format! {"{}{arg}{}",&data.src_before, &data.src_after});
                args.push(comp_path.display().to_string());
                if cfg!(windows) {
                    // only one argument in Windows
                    break;
                }
                comp_path.pop();
            }
        }
    }
}

fn expand_alias(
    aliases: &HashMap<String, Vec<String>>,
    mut cmd: Vec<String>,
    child_env: &HashMap<String, String>,
) -> Vec<String> {
    match aliases.get(&cmd[0]) {
        Some(expand) => {
            let mut interpolated: Vec<String> = Vec::with_capacity(expand.len());
            let mut expand = expand.iter();
            if let Some(element) = expand.next() {
                interpolated.push(element.to_string());
                for element in expand {
                    interpolated.push(interpolate_env(element, child_env))
                }
            }
            cmd.splice(0..1, interpolated);
            cmd
        }
        _ => {
            if cmd[0].starts_with("\\") {
                cmd[0] = cmd[0][1..].to_owned();
            }
            cmd
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
enum EnvExpState {
    #[default]
    TildeCan,
    InArg,
    ExpEnvName,
    InBracketEnvName,
    InEnvName,
    Esc,
    NoInterpol,
    EscNoInterpol,
}

fn interpolate_env(s: &str, child_env: &HashMap<String, String>) -> String {
    // this function called when parameters are going in the processing
    let mut res = String::new();
    let mut state = Default::default();
    let mut curr_env = String::new();

    for c in s.chars() {
        match c {
            '$' => {
                match state {
                    EnvExpState::InArg | EnvExpState::TildeCan => state = EnvExpState::ExpEnvName,
                    EnvExpState::Esc => {
                        state = EnvExpState::InArg;
                        res.push(c)
                    }
                    EnvExpState::InEnvName => {
                        if let Some(v) = child_env.get(&curr_env) {
                            res.push_str(v)
                        } else if curr_env == "0" {
                            res.push_str(TERMINAL_NAME)
                        }
                        curr_env.clear();
                        state = EnvExpState::ExpEnvName
                    }
                    EnvExpState::ExpEnvName => {
                        // current PID
                        res.push_str(&format!("{}", std::process::id()));
                        state = EnvExpState::InArg
                    }
                    EnvExpState::InBracketEnvName => curr_env.push(c),
                    EnvExpState::NoInterpol => res.push(c),
                    EnvExpState::EscNoInterpol => {
                        res.push('\\');
                        res.push(c);
                        state = EnvExpState::NoInterpol
                    }
                }
            }
            '\\' => match state {
                EnvExpState::InArg | EnvExpState::TildeCan => state = EnvExpState::Esc,
                EnvExpState::Esc => {
                    res.push('\\');
                    state = EnvExpState::InArg
                }
                EnvExpState::InEnvName | EnvExpState::ExpEnvName => {
                    if let Some(v) = child_env.get(&curr_env) {
                        res.push_str(v)
                    } else if curr_env == "0" {
                        res.push_str(TERMINAL_NAME)
                    }
                    curr_env.clear();
                    state = EnvExpState::Esc
                }
                EnvExpState::InBracketEnvName => curr_env.push(c),
                EnvExpState::NoInterpol => state = EnvExpState::EscNoInterpol,
                EnvExpState::EscNoInterpol => {
                    res.push(c);
                    state = EnvExpState::NoInterpol
                }
            },
            'a'..='z' | 'A'..='Z' | '_' | '0'..='9' => match state {
                EnvExpState::InArg => res.push(c),
                EnvExpState::TildeCan => {
                    state = EnvExpState::InArg;
                    res.push(c)
                }
                EnvExpState::Esc => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::InEnvName | EnvExpState::InBracketEnvName => {
                    curr_env.push(c);
                }
                EnvExpState::ExpEnvName => {
                    curr_env.push(c);
                    state = EnvExpState::InEnvName
                }
                EnvExpState::NoInterpol => res.push(c),
                EnvExpState::EscNoInterpol => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::NoInterpol
                }
            },
            '~' => {
                match state {
                    EnvExpState::TildeCan => {
                        // expansion can consider another user name after but not implemented yet
                        if let Some(env_value) = env::home_dir() {
                            res.push_str(&env_value.display().to_string())
                        }
                        state = EnvExpState::InArg
                    }
                    EnvExpState::InArg => res.push(c),
                    EnvExpState::Esc => {
                        res.push(c);
                        state = EnvExpState::InArg
                    }
                    EnvExpState::InEnvName => {
                        if let Some(v) = child_env.get(&curr_env) {
                            res.push_str(v)
                        } else if curr_env == "0" {
                            res.push_str(TERMINAL_NAME)
                        }
                        curr_env.clear();
                        if let Some(env_value) = env::home_dir() {
                            res.push_str(&env_value.display().to_string())
                        }
                        state = EnvExpState::InArg
                    }
                    EnvExpState::ExpEnvName => {
                        // $~
                        res.push('$');
                        res.push(c);
                        state = EnvExpState::InArg
                    }
                    EnvExpState::InBracketEnvName => curr_env.push(c),
                    EnvExpState::NoInterpol => res.push(c),
                    EnvExpState::EscNoInterpol => {
                        res.push('\\');
                        res.push(c);
                        state = EnvExpState::NoInterpol
                    }
                }
            }
            '{' => match state {
                EnvExpState::InArg => res.push(c),
                EnvExpState::TildeCan => {
                    state = EnvExpState::InArg;
                    res.push(c)
                }
                EnvExpState::Esc => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::InEnvName => {
                    if let Some(v) = child_env.get(&curr_env) {
                        res.push_str(v)
                    } else if curr_env == "0" {
                        res.push_str(TERMINAL_NAME)
                    }
                    curr_env.clear();
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::ExpEnvName => state = EnvExpState::InBracketEnvName,
                EnvExpState::InBracketEnvName => curr_env.push(c),
                EnvExpState::NoInterpol => res.push(c),
                EnvExpState::EscNoInterpol => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::NoInterpol
                }
            },
            '}' => match state {
                EnvExpState::InArg | EnvExpState::NoInterpol => res.push(c),
                EnvExpState::TildeCan => {
                    state = EnvExpState::InArg;
                    res.push(c)
                }
                EnvExpState::ExpEnvName => {
                    state = EnvExpState::InArg;
                    res.push(c)
                }
                EnvExpState::Esc => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::InEnvName => {
                    if let Some(v) = child_env.get(&curr_env) {
                        res.push_str(v)
                    } else if curr_env == "0" {
                        res.push_str(TERMINAL_NAME)
                    }
                    curr_env.clear();
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::InBracketEnvName => {
                    if let Some(v) = child_env.get(&curr_env) {
                        res.push_str(v)
                    } else if curr_env == "0" {
                        res.push_str(TERMINAL_NAME)
                    }
                    curr_env.clear();
                    state = EnvExpState::InArg
                }
                EnvExpState::EscNoInterpol => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::NoInterpol
                }
            },
            /*'\'' => {
                // no interpolation inside ''
                match state {
                    EnvExpState::InArg | EnvExpState::TildeCan => state = EnvExpState::NoInterpol,
                    EnvExpState::NoInterpol => state = EnvExpState::InArg,
                    EnvExpState::EscNoInterpol => {
                        res.push(c);
                        state = EnvExpState::NoInterpol
                    }
                    EnvExpState::Esc => {
                        res.push(c);
                        state = EnvExpState::InArg
                    }
                    EnvExpState::InBracketEnvName
                    | EnvExpState::InEnvName
                    | EnvExpState::ExpEnvName => (), // generally error
                }
            }*/
            '=' | ':' => match state {
                EnvExpState::NoInterpol => res.push(c),
                EnvExpState::InArg => {
                    state = EnvExpState::TildeCan;
                    res.push(c)
                }
                EnvExpState::TildeCan => {
                    state = EnvExpState::InArg;
                    res.push(c)
                }
                EnvExpState::Esc => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::EscNoInterpol => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::NoInterpol
                }
                EnvExpState::InEnvName | EnvExpState::ExpEnvName => {
                    if let Some(v) = child_env.get(&curr_env) {
                        res.push_str(v)
                    } else if curr_env == "0" {
                        res.push_str(TERMINAL_NAME)
                    }
                    curr_env.clear();
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::InBracketEnvName => curr_env.push(c),
            },
            _ => match state {
                EnvExpState::InArg | EnvExpState::NoInterpol => res.push(c),
                EnvExpState::TildeCan => {
                    state = EnvExpState::InArg;
                    res.push(c)
                }
                EnvExpState::Esc => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::EscNoInterpol => {
                    res.push('\\');
                    res.push(c);
                    state = EnvExpState::NoInterpol
                }
                EnvExpState::InEnvName | EnvExpState::ExpEnvName => {
                    if let Some(v) = child_env.get(&curr_env) {
                        res.push_str(v)
                    } else if curr_env == "0" {
                        res.push_str(TERMINAL_NAME)
                    }
                    curr_env.clear();
                    res.push(c);
                    state = EnvExpState::InArg
                }
                EnvExpState::InBracketEnvName => curr_env.push(c),
            },
        }
    }
    match state {
        EnvExpState::InArg
        | EnvExpState::InBracketEnvName
        | EnvExpState::NoInterpol
        | EnvExpState::TildeCan => {}
        EnvExpState::Esc | EnvExpState::EscNoInterpol => {
            res.push('\\');
        }
        EnvExpState::ExpEnvName => {
            res.push('$');
        }
        EnvExpState::InEnvName => {
            if let Some(v) = child_env.get(&curr_env) {
                res.push_str(v)
            } else if curr_env == "0" {
                res.push_str(TERMINAL_NAME)
            }
        }
    }
    res
}

fn extend_name(arg: &impl AsRef<str>, cwd: &Path, exe: bool) -> String {
    let entered = unescape(arg);
    let mut path = //PathBuf::from(&entered);
        if entered.starts_with('~') { // '~, "~, \~ - no expansion
            if let Some(env_value) = env::home_dir() {
                let res = PathBuf::from(env_value.display().to_string());
                if entered.len() > 1 {
                    res.join(&entered[2..])
                } else {
                    res
                }
            } else {
                PathBuf::from(&entered)
            }
        } else {
            PathBuf::from(&entered)
        };
    //eprintln!("entered: {path:?} {cwd:?}");
    let part_name = path.file_name().unwrap().to_str().unwrap().to_string();
    let dir;
    if path.pop() {
        if path.is_relative() {
            //eprintln!("popped path {:?}", &path);
            if path.as_os_str().is_empty() {
                // join with an empty PathBuf actually add slash because behaves as empty file_name
                dir = cwd.to_path_buf();
            } else {
                dir = cwd.join(path);
            }
        } else {
            dir = path;
        }
    } else {
        dir = cwd.to_path_buf();
    }
    //eprintln!("entered: {cwd:?} {dir:?} {part_name:?}");
    let files: Vec<String> = match dir.read_dir() {
        Ok(read_dir) => read_dir
            .filter_map(|p| {
                p.ok().and_then(|p| {
                    let ep = p.path();
                    let binding = p.file_name();
                    let n = binding.to_string_lossy();
                    if (!exe || ep.is_executable()) && platform_starts_with(&n, &part_name) {
                        let n = n.to_string();
                        if ep.is_dir() {
                            Some(n + MAIN_SEPARATOR_STR)
                        } else {
                            Some(n)
                        }
                    } else {
                        None
                    }
                })
            })
            .collect(),
        Err(_) => vec![],
    };
    let dir = dir.display().to_string();
    //eprintln!("dir: {dir} -> {} for {part_name}", files.len());
    match files.len() {
        0 => format!("{dir}{MAIN_SEPARATOR_STR}{part_name}"),
        1 => format!("{dir}{MAIN_SEPARATOR_STR}{}", &files[0]),
        _ => format!(
            "{dir}{MAIN_SEPARATOR_STR}{}\x07",
            longest_common_prefix(files)
        ),
    }
}

#[cfg(target_os = "windows")]
fn starts_with_ignore_case_ascii(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn platform_starts_with(s: &str, prefix: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        starts_with_ignore_case_ascii(s, prefix)
    }
    #[cfg(unix)]
    s.starts_with(prefix)
}

fn longest_common_prefix(strs: Vec<String>) -> String {
    if strs.is_empty() {
        return String::new();
    }

    let mut prefix = strs[0].clone();
    #[allow(clippy::needless_range_loop)]
    for i in 1..strs.len() {
        let mut j = 0;
        while j < prefix.len()
            && j < strs[i].len()
            && prefix.chars().nth(j) == strs[i].chars().nth(j)
        {
            j += 1;
        }
        prefix = prefix[..j].to_string();
        if prefix.is_empty() {
            break;
        }
    }

    prefix
}

fn remove_redundant_components(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                result.pop();
            }
            _ => result.push(component),
        }
    }
    result
}

pub fn unescape(string: &impl AsRef<str>) -> String {
    let mut res = String::new();
    let mut esc = false;
    for c in string.as_ref().chars() {
        match c {
            '\\' => {
                if esc {
                    esc = false;
                } else {
                    esc = true;
                    continue;
                }
            }
            ':' | ' ' | '!' => {
                esc = false;
            }
            _ => {
                if esc {
                    res.push('\\');
                }
                esc = false
            }
        }
        res.push(c);
    }
    res
}

fn esc_string_blanks(string: String) -> String {
    let mut res = String::new();
    for c in string.chars() {
        match c {
            ' ' | '\\' | '"' | '|' | '(' | ')' | '<' | '>' | ';' | '&' | '$' => res.push('\\'),
            '\'' => res.push('\\'), //; res.push('\\') } // for correct env processing
            _ => (),
        }
        res.push(c);
    }
    res
}

pub fn split_at_star(line: &impl AsRef<str>) -> Option<(String, String)> {
    let char_indices = line.as_ref().char_indices();
    let mut state = Default::default();
    let mut current = String::new();
    let mut before = None;
    for (_, c) in char_indices {
        match c {
            '\\' => match state {
                CmdState::Esc | CmdState::QEsc => current.push(c),
                CmdState::StartArg => state = CmdState::Esc,
                CmdState::InArg => state = CmdState::QEsc,
                _ => unreachable!(),
            },
            '*' => match state {
                CmdState::Esc => {
                    current.push(c);
                    state = CmdState::StartArg
                }
                CmdState::StartArg => {
                    state = CmdState::InArg;
                    before = Some(current.clone());
                    current.clear()
                }
                CmdState::InArg | CmdState::QEsc => {
                    state = CmdState::InArg;
                    current.push(c)
                }
                _ => unreachable!(),
            },
            _ => match state {
                CmdState::Esc => {
                    state = CmdState::StartArg;
                    current.push('\\');
                    current.push(c)
                }
                CmdState::QEsc => {
                    state = CmdState::InArg;
                    current.push('\\');
                    current.push(c)
                }
                CmdState::StartArg | CmdState::InArg => current.push(c),
                _ => unreachable!(),
            },
        }
    }
    match state {
        CmdState::InArg => Some((before.unwrap(), current)),
        CmdState::StartArg | CmdState::Esc => None,
        CmdState::QEsc => {
            current.push('\\');
            Some((before.unwrap(), current))
        }
        _ => unreachable!(),
    }
}

// Windows related

struct EntryLen<'a>(&'a Metadata);

impl fmt::Display for EntryLen<'_> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_dir() || self.0.is_symlink() {
            "".fmt(fmt)
        } else {
            self.0.len().fmt(fmt)
        }
    }
}

#[allow(clippy::upper_case_acronyms)]
enum Op {
    DEL,
    CPY,
    REN,
    TYP,
}
#[derive(Debug, Default)]
struct DeferData {
    src: PathBuf,
    src_before: String,
    src_after: String,
    src_wild: Vec<String>,
    dst: Option<PathBuf>,
    dst_before: Option<String>,
    dst_after: Option<String>,
    // not for the Rust version
    //defer_op: Option<Op>,
}
use std::path::Path;
impl DeferData {
    fn from(cwd: &Path, from: &Path) -> DeferData {
        let from_name = from.file_name().unwrap_or_default().display().to_string();
        let from_dir = from.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let dir = if from_dir.has_root() {
            from_dir
        } else {
            cwd.join(&from_dir)
        };
        let (src_wild, src_before, src_after) = match split_at_star(&from_name) {
            None => (vec![from_name], String::new(), String::new()),
            Some((before, after)) => (
                dir.read_dir()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| {
                        r.ok().and_then(|e| {
                            let s = e.file_name().display().to_string();
                            if s.len() >= before.len() + after.len() {
                                s.strip_prefix(&before)
                                    .and_then(|name| name.strip_suffix(&after))
                                    .map(str::to_string)
                            } else {
                                None
                            }
                        })
                    })
                    .collect(),
                before,
                after,
            ),
        };
        DeferData {
            src: dir,
            src_before,
            src_after,
            src_wild,
            ..Default::default()
        }
    }

    fn from_to(cwd: &Path, from: &Path, to: &Path) -> Self {
        let mut res = DeferData::from(cwd, from);
        let to_name;
        let to = if !to.has_root() {
            cwd.join(to)
        } else {
            to.to_path_buf()
        };
        let to_dir = if to.is_dir() {
            to_name = String::new();
            to
        } else {
            to_name = to.file_name().unwrap().to_str().unwrap().to_string();
            to.parent().unwrap_or(&PathBuf::from("")).to_path_buf() // ??? the code needs review in case of no parent
        };
        //
        let (to_before, to_after) = match split_at_star(&to_name) {
            None => {
                // no wild card
                (None, None)
            }
            Some((before, after)) => (Some(before.to_string()), Some(after.to_string())),
        };
        res.dst = Some(to_dir);
        res.dst_before = to_before;
        res.dst_after = to_after;
        //eprintln!("from {res:?}");
        res
    }

    fn do_op(&mut self, op: Op) -> io::Result<u32> {
        let mut succ_count = 0;
        let file = &mut self.src;
        for name in &self.src_wild {
            let name_to = if self.dst.is_some()
                && let Some(dst_before) = &self.dst_before
                && let Some(dst_after) = &self.dst_after
            {
                format! {"{dst_before}{name}{dst_after}"}
            } else {
                String::new()
            };
            let name = format! {"{}{name}{}",&self.src_before, &self.src_after};
            //eprintln!{"{:?} to {:?} {name} to {name_to:?}", self.src, self.dst}
            file.push(&name);
            match op {
                Op::TYP => {
                    //eprintln!{"typing: {file:?}"}
                    let contents = fs::read_to_string(&file)?;
                    send!("{}", contents);
                    succ_count += 1
                }
                Op::DEL => {
                    if file.is_file() && fs::remove_file(&file).is_ok()
                        || file.is_dir() && fs::remove_dir_all(&file).is_ok()
                    {
                        succ_count += 1
                    }
                }
                Op::CPY => {
                    let dest = self.dst.as_mut().unwrap();
                    if !name_to.is_empty() {
                        dest.push(&name_to)
                    } else {
                        dest.push(name) // 
                    }
                    if file.is_file() {
                        if fs::copy(&file, &dest).is_ok() {
                            succ_count += 1
                        };
                    } else if file.is_dir()
                        && let Ok((files, _)) = copy_directory(file, dest, &true)
                    {
                        succ_count += files
                    }
                    //if !name_to.is_empty() {
                    dest.pop();
                    //}
                }
                Op::REN => {
                    let dest = self.dst.as_mut().unwrap();
                    let overwrite = true;
                    if !name_to.is_empty() {
                        dest.push(&name_to)
                    }
                    if file.is_file() || file.is_dir() {
                        if let Err(err) = fs::rename(&file, &dest)
                            && err.kind() == ErrorKind::CrossesDevices
                        {
                            if file.is_file() && (overwrite || !dest.exists()) {
                                // probably not rquired
                                if fs::copy(&file, &dest).is_ok() {
                                    let _ = fs::remove_file(&file);
                                }
                            } else if file.is_dir() {
                                match copy_directory(file, dest, &overwrite) {
                                    Ok(cnt) => {
                                        // TODO decide of cases when only some files were copied
                                        let _ = fs::remove_dir_all(&file);
                                        succ_count += cnt.0
                                    }
                                    Err(_err) => (),
                                }
                            }
                        } else {
                            // eprintln!{"renaming {file:?} to {dest:?}"}
                            succ_count += 1
                        }
                    }
                    if !name_to.is_empty() {
                        dest.pop();
                    }
                }
            }
            file.pop();
        }
        Ok(succ_count)
    }
}

fn copy_directory(
    source_dir: &Path,
    destination_dir: &Path,
    overwrite: &bool,
) -> io::Result<(u32, u64)> {
    fs::create_dir_all(destination_dir)?; // Create the destination directory if it doesn't exist
    let mut count = 0u32;
    let mut size = 0u64;
    for entry in fs::read_dir(source_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let file_name = path
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid file name"))?;
            let dest_path = destination_dir.join(file_name);
            if !*overwrite && dest_path.exists() {
                return Err(io::Error::other(format!(
                    "destination {dest_path:?} exists"
                )));
            }
            size += fs::copy(&path, &dest_path)?;
            count += 1
        } else if path.is_dir() {
            let file_name = path
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid file name"))?;
            let dest_path = destination_dir.join(file_name);
            match copy_directory(&path, &dest_path, overwrite) {
                Ok((files, copied_size)) => {
                    count += files;
                    size += copied_size
                }
                Err(err) => return Err(err),
            }
        }
    }
    Ok((count, size))
}

fn emulate_unix_cmd(cmd: &[String], cwd: &Path) -> io::Result<Option<Vec<u8>>> {
    match cmd[0].as_str() {
        "echo" => {
            if cmd.len() != 2 {
                return Err(io::Error::other("Wrong number of 'echo' arguments"));
            }
            Ok(Some(cmd[1].as_bytes().to_vec()))
        }
        "type" => {
            if cmd.len() != 2 {
                return Err(io::Error::other("Wrong number of 'type' arguments"));
            }
            // wild card Windows specific
            let mut data = DeferData::from(cwd, &PathBuf::from(&cmd[1]));
            let mut contents = String::with_capacity(4 * 1024);
            for arg in data.src_wild {
                data.src
                    .push(format! {"{}{arg}{}",&data.src_before, &data.src_after});
                contents.push_str(&fs::read_to_string(&data.src)?);
                data.src.pop();
            }
            Ok(Some(contents.as_bytes().to_vec()))
        }
        "dir" => {
            let names_only = cmd.len() > 1 && cmd[1] == "/b";
            let mut dir = if cmd.len() == if names_only { 2 } else { 1 } {
                cwd.to_path_buf().join("*")
            } else {
                PathBuf::from(&cmd[if names_only { 2 } else { 1 }])
            };
            let data = DeferData::from(cwd, &dir);
            let mut res = String::new();
            for arg in data.src_wild {
                dir.push(format! {"{}{arg}{}",&data.src_before, &data.src_after});
                if let Some(file_name) = dir.as_path().file_name() {
                    res.push_str(&file_name.display().to_string());
                    res.push('\n');
                }
                dir.pop();
            }
            Ok(Some(res.as_bytes().to_vec()))
        }
        _ => Ok(None),
    }
}
