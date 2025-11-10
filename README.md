# Simple Terminal

## Purpose
Provide a terminal functionality in a Web or a standalone application.

## How to use
1. Implement trait `Terminal`. Only `init` is required. For example:
```Rust
struct Commander {}

impl Terminal for Commander {
    fn init(&self) -> (PathBuf, PathBuf, HashMap<String,Vec<String>>,&str) {
        let web = simweb::WebData::new();
        let os_drive =
            if "windows" == consts::OS {
                match env::var("SystemDrive") {
                    Ok(value) => value,
                    Err(_e) => String::new(),
                }
            } else {
                 String::new()
            };
        let cwd = match web.param("cwd") {
            Some(cwd) => PathBuf::from(cwd),
            _ => PathBuf::from(format!("{os_drive}{}", std::path::MAIN_SEPARATOR_STR))
        };
        (cwd.clone(),cwd,HashMap::new(),VERSION)
    }
}
```
2. Call `main_loop` in the `main` app function, like:
```Rust
fn main() {
    let _ = Commander{}.main_loop();
}
```
3. Client part should include *terminal.js*, and then use a code like:
```JavaScript
function openTerminal() {
    const dirInputLeft = document.getElementById(`left-dir`);
    const actPanel = dirInputLeft.classList.contains('selected-panel')?'left':'right';
    const div = document.createElement('DIV');
    div.id = "terminal"
    div.tabindex = 0
    const dir = document.getElementById(`${actPanel}-dir`).value
    const code = document.createElement('CODE')
    code.contentEditable = true
    code.id = 'commandarea'
    code.style = "min-width:1em"
    code.addEventListener('keydown', function() { sendCommand(code) })
    code.textContent = '\xa0'
    div.appendChild(code)
    document.body.appendChild(div)
    code.focus()
    document.title = 'Terminal'
    WS_TERM_URL = `${WS_TERM_URL_BASE}?cwd=${encodeURIComponent(dir)}`
    ws_term_connect()
}
```
4. The following JSON code snippet has to be added in [SimHTTP](https://github.com/vernisaz/simhttp) configuration:
```JSON
{"path":"/cmd/js",
   "translated": "./html/js"},

{"path":"/cmd/term",
   "WS-CGI": true,
   "translated": "./bin/cmdterm"}
```
Actual mapping values will depend on your desired settings.

## How build the crate
Use [RustBee](https://github.com/vernisaz/rust_bee) for that. The built crate will be stored in *../crates* directory.
You can also use Cargo.
