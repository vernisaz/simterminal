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
const WIN_SERVER = true
function openTerminal() {
    const dirInputLeft = document.getElementById(`left-dir`);
    const actPanel = dirInputLeft.classList.contains('selected-panel')?'left':'right';
    const dir = document.getElementById(`${actPanel}-dir`).value
    const container = document.createElement('DIV');
    container.id = 'terminal-container'
    container.className = 'scroll-up'
    const div = document.createElement('DIV');
    div.id = "terminal"
    div.tabindex = 0
    const code = document.createElement('CODE')
    code.contentEditable = true
    code.id = 'commandarea'
    code.style = "min-width:1em"
    code.addEventListener('keydown', function() { sendCommand(code) })
    code.textContent = '\xa0'
    div.appendChild(code)
    container.appendChild(div)
    document.body.appendChild(container)
    code.focus()
    document.title = 'Terminal'
    WS_TERM_URL = `${WS_TERM_URL_BASE}?cwd=${encodeURIComponent(dir)}`
    ws_term_connect()
}
function closeTerminal() { // optionally, add it for 'exit' like command processing
    ws_term_close()
    // ... some other actions
}
```
4. Add CSS
```CSS
.scroll-up {
    opacity:0.9;
    background-color:#ddd;
    position:fixed;
    width:100%;
    height:100%;
    top:0px;
    left:0px;
    overflow:auto;
    z-index:998;
}

div#terminal {
    padding: 1em;
    color: #0e131f;
    font-family: monospace; 
}

#terminal pre {
    display: inline;
}
```
5. The following JSON code snippet has to be added in [SimHTTP](https://github.com/vernisaz/simhttp) [configuration](https://github.com/vernisaz/simhttp/blob/master/env.conf):
```JSON
{"path":"/cmd/js",
   "translated": "./html/js"},

{"path":"/cmd/term",
   "WS-CGI": true,
   "translated": "./bin/cmdterm"}
```
`cmdterm` is the name of the executable created in step 2. Actual mapping values will depend on your desired settings.


## How build the crate
Use [RustBee](https://github.com/vernisaz/rust_bee) for that. The built crate will be stored in *../crates* directory.
You can also use Cargo.
