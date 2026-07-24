# Simple Terminal

## Purpose
Provide a terminal functionality for a web or a standalone application.

## How to use
A web server must support the websocket CGI extention endpoints. For example, [SimHTTP](https://github.com/vernisaz/simhttp) supports WS-CGI.

1. Implement trait `Terminal`. Only `init` is required. For example:
```Rust
struct Commander ;
const VERSION: &str = "1.1.1";

impl Terminal for Commander {
    fn init(&self) -> (PathBuf, PathBuf, HashMap<String,String>,&str) {
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
2. Call `main_loop` in the `main` endpoint function, like:
```Rust
fn main() {
    let _ = Commander.main_loop();
}
```
3. A client part should include *terminal.js*, and then use a code, like:
```JavaScript
const WIN_SERVER = true
const WS_TERM_URL_BASE = './term'
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

The dynamically added the terminal HTML can be replaced by a static equivalent:
```html
<section id="terminal-container">
    <div id="terminal">
        <code contenteditable="true" id="commandarea" onkeydown="sendCommand(this)" autofocus style="min-width:1em">&nbsp;</code>
    </div>
</section>
```

4. Add CSS
```CSS
#terminal {
    color: #0e131f;
    font-family: monospace; 
    padding-top:0.5em;
    padding-bottom: 1em;
    width: fit-content;
}

#terminal pre {
    display: inline;
}

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

.scroll-up > div {
    padding: 1em;
    color: #0e131f;
    font-family: monospace; 
}

.scroll-up > div pre {
    display: inline;
}
@keyframes blink {
	0% { opacity:1 } 75% { opacity:1 } 76% { opacity:0 } 100% { opacity:0 }
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

`cmdterm` is the name of the executable created in step 2. Actual mapping values will depend on your preferred settings.

`terminal.js` should be stored in _./html/js_ directory.

6. Customize terminal output by adding clickable links

Sometimes a terminal output can contain URLs and other clickable elements as references to a source code with line numbers. Such
elements can be wrapped in clickable links in the terminal output. Define function `extendURL` with a string parameter
returning a string with embedded URLs (if applied), for example:
```JavaScript
var fileNameReg
if (WIN_SERVER)
    fileNameReg =  /(?<path>(\w:\\)?((\w+|\.\.?)\\)*)(?<file>\w+\.(rs|swift|java)):(?<line>\d+):(?<col>\d+|\s)/gm
else
    fileNameReg = /(?<path>\/?((\w+|\.\.?)\/)*)(?<file>\w+\.(rs|swift|java)):(?<line>\d+):(?<col>\d+|\s)/gm
function extendURL(lineStr) {
    return lineStr.replaceAll(fileNameReg, (match) => {
        const matchGroup = [...match.matchAll(fileNameReg)]
        const file = matchGroup[0].groups.file;
        const line = matchGroup[0].groups.line;
        const col = matchGroup[0].groups.col;
        var path = matchGroup[0].groups.path
        path = path.replaceAll('\\', '/')
        return `<a href="javascript:moveToLineInFile('${path}${file}',${line},${col})">${match}</a>`
    });
}
```
Optionally add CSS to avoid a decoration of links:
```CSS
span+a,pre a {
  color: inherit;
  text-decoration: inherit;
}
```

## Handling pipe and redirection
The terminal emulator acknowledges `|` - pipe and redirection `<>` symbols. Since some Windows commands do not have a corresponding
executable, the **SimTerminal** emulates their work on the best assumption.

## How build the crate
Use [RustBee](https://github.com/vernisaz/rust_bee) for that. The built crate will be stored in *../crates* directory by default.
You can also use Cargo.
The three dependency crates are:
- The [Simple Time](https://github.com/vernisaz/simtime)
- The [SimWeb](https://github.com/vernisaz/simweb)
- The [Simple Color](https://github.com/vernisaz/simcolor)
- And the [Common building scripts](https://github.com/vernisaz/simscript) for building them

## Where it is used
- [Simple commander](https://github.com/vernisaz/simcom) file manager
- [Rust Development Studio](https://github.com/vernisaz/rust_dev_studio) multipurpose web IDE

## Known problem
The terminal can't be dismissed if it starts an app which continues running. Use `kill` for the terminal to terminate it.
