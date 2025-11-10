// source https://i.sstatic.net/9UVnC.png
const PAL_LOOKUP = ["rgb(12,12,12)", "rgb(197,15,31)", "rgb(19,161,14)", "rgb(193,156,0)", "rgb(0, 71, 171)",
                    "rgb(136,23,152)", "rgb(58,150,221)", "rgb(204,204,204)", "rgb(118,118,118)", "rgb(231,72,86)",
                    "rgb(22,198,12)", "rgb(246,234,158)", "rgb(59,120,255)", "rgb(180,0,158)", "rgb(97,214,214)", "rgb(242,242,242)"]
// "rgb(250,229,135)"
var lastChunk = ''

/* */
var bold = false
var under = false
var revs = false
var strike = false
var italic = false
var hide = false
var blink = false
var fon_color = ''
var fon_back = ''
/*  */            
            
function clearColorAttributes() {
    /* */
    bold = false
    under = false
    revs = false
    strike = false
    italic = false
    hide = false
    blink = false
    fon_color = ''
    fon_back = ''
     /*                   */
}

var termWskt
var notifRecon = 0
var maxReconn = 16 * 1000
const commandBuffer = []
var cmdBufPos = -1

function ws_term_connect() {
    termWskt = new WebSocket(WS_TERM_URL)
    termWskt.onopen = function(d) {
         notifRecon = 500
    }
    termWskt.onmessage = function(e) {
         
        if (e.data.startsWith('\r') && (e.data.length == 1 || e.data.charAt(1) != '\n')) { // not very relaible now since can interfere with a regular out
            const cmd = document.getElementById('commandarea')
            var cmdStr
            if (e.data.slice(-1) == '\x07') {
                cmdStr = e.data.slice(1, -1).trim();
                beep()
            } else {
                cmdStr = e.data.trim()
            }
            if (cmdStr)
                    cmd.innerHTML = htmlEncode(cmdStr)
            cmd.focus()
            document.execCommand('selectAll', false, null)
            document.getSelection().collapseToEnd()
            return
        }
        
        const cons = document.querySelector('#terminal')
        var noPrompt = true
        
        //console.log(e.data)  // for debug
        // the code handles the situation when data split between two chunks (not more than two though)
        var chunk = e.data
        if (chunk.charAt(chunk.length - 1) == '\f') {
            noPrompt = false
            chunk = chunk.slice(0, -1)
        }
  
        if (chunk.charAt(chunk.length - 1) == '\x1b') {
             chunk = lastChunk + chunk.substring(0, chunk.length-1)
             lastChunk = '\x1b'
        } else if (chunk.length >= 2 && chunk.charAt(chunk.length - 2) == '\x1b' && chunk.charAt(chunk.length - 1) == '[') {
             chunk = lastChunk + chunk.substring(0, chunk.length-2)
             lastChunk = '\x1b['
        } else {
            const lastEsc = chunk.lastIndexOf('\x1b[')
            if (lastEsc > 0 && chunk.indexOf('m', lastEsc) < 0) {
                const over = chunk.substring(lastEsc)
                chunk = lastChunk + chunk.substring(0, lastEsc)
                lastChunk = over
            } else {
                if (lastChunk) {
                    chunk = lastChunk + chunk
                    lastChunk = ''
                }
            }
        }
        if ( chunk . startsWith('\x1b[') && chunk.indexOf('m') < 0) {
            lastChunk = chunk
            return
        }
        var wasEsc = chunk . startsWith('\x1b[')
        const ansi_esc = chunk.split(/\x1b\[/g)
        const term_frag = document.createElement("pre")
        if (ansi_esc.length > 1) {
            var ansi_html = ''
            // assure esc[0m when stream closed on the endpoint side
            var shift
            for (var ans of ansi_esc) {
                // procceed ANSI code
                shift = 0
                //if (false && wasEsc) {
                if (wasEsc) {
                do {
                    if (ans.charAt(shift) == '0' || ans.charAt(shift) == 'm') { // reset
                        clearColorAttributes()
                        if (ans.charAt(shift) != 'm')
                           shift ++
                    } else if (ans.charAt(shift) == '9') {
                        if (ans.charAt(shift+1) >= '0' && ans.charAt(shift+1) <= '9') {
                            fon_color = PAL_LOOKUP[Number(ans.charAt(shift + 1)) + 8]
                            shift += 2
                        } else { // ; or m
                            strike = true
                            shift++
                        }
                    } else if (ans.charAt(shift) == '4' && ans.charAt(shift + 1) != ';' && ans.charAt(shift + 1) != 'm' && ans.charAt(shift + 1) != '8'  && ans.charAt(shift + 1) != '9') {
                        fon_back = PAL_LOOKUP[ans.charAt(shift + 1)]
                        shift += 2
                    } else if ( ans.charAt(shift) == '1' && ans.charAt(shift + 1) == '0') {
                        fon_back = PAL_LOOKUP[Number(ans.charAt(shift + 2)) + 8]
                        shift += 3
                    }  else if (ans.charAt(shift) == '1') {
                        bold = true
                        shift += 1
                    } else if (ans.charAt(shift) == '7') { // investigate how manage dark theme
                        fon_color = 'Canvas'
                        fon_back = 'CanvasText'
                        shift += 1
                    } else if (ans.charAt(shift) == '2' && ans.charAt(shift+1) == '4') {
                        under = false
                        shift += 2
                    } else if (ans.charAt(shift) == '2' && ans.charAt(shift+1) == '7') {
                        fon_back = 'Canvas'
                        fon_color = 'CanvasText'
                        shift += 2
                    } else if (( ans.charAt(shift) == '3' || ans.charAt(shift) == '4') && (ans.length-shift) > 5 && ans.charAt(shift + 1) == '8') {
                        if (ans.charAt(shift + 2) == ';' && ans.charAt(shift + 3) == '5' &&
                            ans.charAt(shift + 4) == ';') {
                            const forg = ans.charAt(shift) == '3'
                            shift += 5
                            // find 'm' and get number
                            var colNum 
                            if (ans.charAt(shift + 1) == 'm') {
                                colNum = ans.charAt(shift)
                                shift += 1
                            } else if (ans.charAt(shift + 2) == 'm') {
                                colNum = Number(ans.substring(shift, shift+2))
                                shift += 2
                            }
                            if (colNum > -1 && colNum < PAL_LOOKUP.length) {
                                if (!(WIN_SERVER && colNum == PAL_LOOKUP.length - 1)) {
                                    if (forg)
                                        fon_color = PAL_LOOKUP[colNum]
                                    else
                                        fon_back = PAL_LOOKUP[colNum]
                                }
                            } else
                                shift = 0
                        } else
                            shift = 0
                    } else if (ans.charAt(shift) == '4') {
                        if (ans.charAt(shift+1) >= '0' && ans.charAt(shift+1) <= '9') {
                            if (ans.charAt(shift+1) == '9') {
                                fon_back = ''
                            }
                            shift += 2
                        } else {
                            under = true
                            shift += 1
                        }
                    } else if (ans.charAt(shift) == '3') {
                        if (ans.charAt(shift + 1) >= '0' && ans.charAt(shift + 1) <= '7') {
                            fon_color = PAL_LOOKUP[ans.charAt(shift + 1)]
                            shift += 2
                        } else if (ans.charAt(shift + 1) == '9') {
                            fon_color = '' 
                            shift += 2
                        } else {
                            italic = true
                            shift += 1
                        }
                    } else if (ans.charAt(shift) == '5' || ans.charAt(shift) == '6') {
                        if (ans.charAt(shift+1) >= '0' && ans.charAt(shift+1) <= '9') {
                            shift += 2
                        } else {
                            blink = true
                            shift += 1
                        }
                    } else if (ans.charAt(shift) == '8') {
                        if (ans.charAt(shift+1) >= '0' && ans.charAt(shift+1) <= '9') {
                            shift += 2
                        } else {
                            hide = true
                            shift += 1
                        }
                    } else 
                        shift = 0
                    if (shift != 0 && ans.charAt(shift) == ';')
                        shift += 1
                    //console.log('shift'+shift)

                } while (ans.charAt(shift) != 'm' && shift != 0 && shift < ans.length)
                }
                const applyFmt = fon_color || fon_back || bold || under || strike || italic || blink || hide

                if ((!wasEsc || shift > 0) && ans.length > shift || applyFmt) {
                    if (applyFmt)  {
                        ansi_html += '<span style="'
                        if (fon_color ) 
                             ansi_html += 'color:' + fon_color + ';'
                        if (fon_back ) 
                             ansi_html += 'background-color:' + fon_back + ';'
                        if (bold ) 
                             ansi_html += 'font-weight: bold;'
                        if (under ) 
                             ansi_html += 'text-decoration: underline;'
                        if ( strike )
                            ansi_html += 'text-decoration: line-through;'
                       if ( italic )
                            ansi_html += 'font-style: italic;'
                        if ( blink )
                            ansi_html += 'animation:blink 0.75s ease-in infinite alternate!important;'
                        if ( hide )
                            ansi_html += 'opacity: 0.0;'
                        ansi_html += '">' + htmlEncode(ans.substring(shift>0?shift + 1:0)) +'</span>'
                    } else {
                        var fileNameReg
                        if (WIN_SERVER)
                            fileNameReg =  /(?<path>(\w:\\)?((\w+|\.\.)\\)*)(?<file>\w+\.(rs|swift)):(?<line>\d+):(?<col>\d+)/gm
                        else
                            fileNameReg = /(?<path>\/?((\w+|\.\.)\/)*)(?<file>\w+\.(rs|swift)):(?<line>\d+):(?<col>\d+)/gm // TODO introduce path
                        const lineStr = htmlEncode(ans.substring(shift>0?shift + 1:0))
                       // const matches = lineStr.matchAll(fileNameReg); 
                        const matches = Array.from(lineStr.matchAll(fileNameReg)); // [...matchAll]
                       //if (false) {
                        if (matches.length > 0) {
                            const file = matches[0].groups.file;
                            const line = matches[0].groups.line;
                            const col = matches[0].groups.col;
                            var path = matches[0].groups.path
                            if (path.startsWith('/') || path.indexOf(':\\') == 1) // current OS root
                                path = path.substring(HOME_LEN + PROJECT_HOME.length+1)
                            path = path.replaceAll('\\', '/')
                            ansi_html += `<a href="javascript:moveToLineInFile('${path}${file}',${line},${col})">${lineStr}</a>`
                        } else {
                           ansi_html += lineStr //htmlEncode(ans.substring(shift>0?shift + 1:0))
                        }
                    }
                } else {
                    if (ans.charAt(shift) == 'm')
                        shift ++
                    if (ans.length > shift) // TODO refactor
                         ansi_html += htmlEncode(ans.substring(shift))
                }
                wasEsc = true
            }
            //console.log(ansi_html) // debug
            term_frag.innerHTML = ansi_html
        } else
            term_frag.innerHTML = htmlEncode(chunk)
        //cons.appendChild(term_frag)
        appendContent(cons,term_frag)
        if (!noPrompt) {
            // print command prompt
            const prompt = document.createElement("pre")
            prompt.textContent = '\n$'
            appendContent(cons,prompt)
            //cons.appendChild(prompt)
        }
        cons.scrollIntoView({ behavior: "smooth", block: "end" })
     }
     termWskt.onclose = (event) => {
        if (notifRecon == 0)
          notifRecon = 500
        if (notifRecon < maxReconn)
          notifRecon *= 2
        if (console && console.log)
            console.log(`Oops, ${event}  reconnecting in ${notifRecon}ms because ${event.reason}`)
        setTimeout(ws_term_connect, notifRecon)
     }
}

function appendContent(term,el) {
    const lastChild = term.lastElementChild;
    term.insertBefore(el, lastChild);
}

function sendCommand(cmd) {
   switch (event.key) {
    case 'Enter':
         if (termWskt && termWskt.readyState===WebSocket.OPEN) {
           if (cmd.textContent && cmd.textContent != '\xa0') {
               if (cmd.textContent.trim() == 'clear' || WIN_SERVER && cmd.textContent.trim() == 'cls') { // 'reset'
                   clearScreen()
               } else {
                    let inputStr = cmd.textContent.trim()
                    if (inputStr.startsWith('\xa0'))
                        inputStr = inputStr.substring(1)
                    if (inputStr.endsWith('\xa0'))
                        inputStr = inputStr.substring(0, inputStr.length-1)
                     
                    termWskt.send(inputStr+'\n')
               }
               const commIdx = commandBuffer.indexOf(cmd.textContent)
               if (commIdx < 0)
                    commandBuffer.push(cmd.textContent)
               else
                    cmdBufPos = commIdx
		   } else
		  	sendEnter()
		  cmd.textContent = '\xa0'
		  event.preventDefault()
	   } else {
	         console.log('websocket closed')  
	         alert('The server ' + ws_url + ' is currently unreachable')
	   }
        return
    case 'ArrowUp':
        if (commandBuffer.length) {
           cmdBufPos--
           if (cmdBufPos < 0)
              cmdBufPos = commandBuffer.length-1
        }
        break
    case 'ArrowDown':
        if (commandBuffer.length) {
           cmdBufPos++
           if (cmdBufPos > commandBuffer.length-1)
              cmdBufPos = 0 
        }
        break
    case 'Tab':  // event.keyCode 9 
        termWskt.send(cmd.textContent + '\t')
        event.preventDefault()
        return
    case 'Escape':
        cmd.textContent = '\xa0'
        event.preventDefault()
        return
    case "Backspace":
    case "Delete":
        if (cmd.textContent.length == 1) {// prevent complete cleaning the field
            event.preventDefault()
            cmd.textContent = '\xa0'
        }
        return
    default:
       if (event.ctrlKey) {
          if (event.keyCode == 67) {
       	   sendCtrlC()
       	   event.preventDefault()
          } else if (event.keyCode == 90){
          	sendCtrlZ()
          	event.preventDefault()
          } else if (event.keyCode == 76) {
         	 clearScreen()
         	 event.preventDefault()
          } 
       }
       return
    }
    if (commandBuffer.length) {
	 	cmd.innerText = commandBuffer[cmdBufPos]
	    const range = document.createRange();
        const selection = window.getSelection();

        range.selectNodeContents(cmd);
        range.collapse(false); // Collapse to the end

        selection.removeAllRanges();
        selection.addRange(range);

        cmd.focus();
    }
    event.preventDefault()
}

function sendEnter() {
   if (termWskt && termWskt.readyState===WebSocket.OPEN) {
	    termWskt.send('\n')
	}
}
function sendCtrlZ() {
   if (termWskt && termWskt.readyState===WebSocket.OPEN) {
	   termWskt.send('\u001A')
	   document.querySelector('#commandarea').textContent='\xa0'
   }
}
function sendCtrlC() {
   if (termWskt && termWskt.readyState===WebSocket.OPEN)
	          termWskt.send('\x03')
}
function clearScreen() {
    const cons = document.querySelector('#terminal')
   //cons.replaceChildren();
    clearColorAttributes()
    while (cons.firstChild.tagName != 'CODE') {
        cons.firstChild.remove()
    }
    const prompt = document.createElement("pre")
    prompt.textContent = '$'
    appendContent(cons,prompt)
}