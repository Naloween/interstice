#!/usr/bin/env bash
# Regression tests for the browser example, driven through its scripted-input
# hook: each scenario loads a `data:` page (no network), replays a
# `browser_script` (see `Browser::script` in src/engine.rs) and checks the
# `browser-test` lines the `dump` / `js` commands log.
#
# Usage: modules/examples/browser/tests/headless.sh [scenario…]
# Needs the current modules and a release CLI built from this checkout (it
# runs the example from dist/examples):
#   scripts/build_modules.sh
#   cargo build -p interstice-cli --release
# The window is 1280x900; the content area starts at y = 85. With `xvfb-run`
# installed (`sudo apt install xvfb`) it opens on a virtual display, so the
# tests neither pop up on the desktop nor take its keyboard input; set
# HEADLESS_DISPLAY=host to use the real display anyway.
set -u
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
BIN="$ROOT/target/release/interstice"
TIMEOUT="${TIMEOUT:-20}"
[ -x "$BIN" ] || { echo "missing $BIN (cargo build -p interstice-cli --release)"; exit 2; }

# Where the windows open (see above).
if [ "${HEADLESS_DISPLAY:-}" != host ] && command -v xvfb-run > /dev/null; then
    WRAP=(env -u WAYLAND_DISPLAY xvfb-run -a -s "-screen 0 1280x900x24")
else
    WRAP=()
    [ "${HEADLESS_DISPLAY:-}" = host ] || echo "note: windows open on the desktop (install xvfb to avoid it)"
fi

page() { printf 'data:text/html;base64,%s' "$(printf '%s' "$1" | base64 -w0)"; }

fails=0
# run NAME HTML SCRIPT EXPECT — EXPECT lines: "<frame> <substring>"; the
# `browser-test frame=<frame>` line must contain the substring.
run() {
    local name=$1 html=$2 script=$3 expect=$4
    local dir; dir=$(mktemp -d)
    page "$html" > "$dir/browser_home"
    printf '%s\n' "$script" > "$dir/browser_script"
    (cd "$dir" && INTERSTICE_WINDOW_SIZE=1280x900 "${WRAP[@]}" timeout "$TIMEOUT" "$BIN" example browser > log 2>&1)
    local ok=1
    while read -r frame want; do
        [ -z "$frame" ] && continue
        local line; line=$(grep -F "browser-test frame=$frame " "$dir/log" | head -1)
        if [[ "$line" != *"$want"* ]]; then
            ok=0
            echo "  FAIL $name @$frame: want [$want]"
            # Show just the field being checked (`key=…`), or a note if absent.
            local key=${want%%=*} got
            got=$(grep -o " $key=[^ ]*\| $key=\"[^\"]*\"" <<< "$line" | head -1)
            if [ -z "$line" ]; then got="(no browser-test line)"; fi
            echo "       got  [${got:-(no $key in line)}]"
        fi
    done <<< "$expect"
    if grep -q "browser-test: \|panicked" "$dir/log"; then
        ok=0
        grep "browser-test: \|panicked" "$dir/log" | sed 's/^/  /' | head -5
    fi
    if [ $ok = 1 ]; then echo "ok   $name"; rm -rf "$dir"; else echo "     (log: $dir/log)"; fails=$((fails + 1)); fi
}

only=("$@")
selected() { [ ${#only[@]} = 0 ] || [[ " ${only[*]} " == *" $1 "* ]]; }

# ── Scrolling: wheel and keyboard, clamped at both ends ──────────────────────
# 100 × 100px paragraphs = 10000px; viewport 815px → max scroll 9185.
# A page is 85% of the viewport (692.75px).
if selected scroll; then
paras=$(for i in $(seq 1 100); do printf '<p style="height:100px;margin:0">%d</p>' "$i"; done)
run scroll "<body style=\"margin:0\">$paras</body>" "
60 click 600 400
70 dump
80 wheel 0 300
90 dump
100 key ArrowDown
110 dump
120 key PageDown
130 dump
140 key End
150 dump
160 key Home
170 dump
180 key Space
190 dump
200 key Shift+Space
210 dump
220 wheel 0 -500
230 dump" "
70 scroll=0,0
90 scroll=0,300
110 scroll=0,348
130 scroll=0,1041
150 scroll=0,9185
170 scroll=0,0
190 scroll=0,693
210 scroll=0,0
230 scroll=0,0"
fi

# ── Keyboard: focus, typing, editing, Tab order, script key events ──────────
if selected keyboard; then
run keyboard '<body style="margin:0">
<input id="q" style="position:absolute;left:10px;top:10px;width:300px;height:30px">
<input id="r" style="position:absolute;left:10px;top:60px;width:300px;height:30px">
<div style="height:3000px"></div>
<script>window.k=[];document.addEventListener("keydown",function(e){k.push(e.key)});</script>
</body>' "
60 click 100 110
70 dump
80 type Hello world
90 js document.getElementById('q').value
91 js k.join(',')
100 key Backspace
110 js document.getElementById('q').value
120 key Tab
130 dump
140 type x
150 js document.getElementById('r').value
160 key ArrowDown
170 dump" "
70 focus=input#q
90 js=\"Hello world\"
91 js=\"Shift,H,e,l,l,o, ,w,o,r,l,d\"
110 js=\"Hello worl\"
130 focus=input#r
150 js=\"x\"
170 scroll=0,0"
fi

# ── Chrome: URL bar editing and navigation, tabs ─────────────────────────────
if selected chrome; then
run chrome '<h1>one</h1>' "
60 key Control+KeyL
70 dump
80 type about:newtab
90 dump
100 key Enter
160 dump
170 key Control+KeyT
180 dump
190 key Control+KeyW
200 dump" "
70 url_editing=true
90 url_text=\"about:newtab\"
160 url=\"about:newtab\"
160 url_editing=false
180 tabs=2 active=1
200 tabs=1"
fi

# ── Script events: click listeners, preventDefault on links ──────────────────
if selected events; then
run events '<body style="margin:0">
<div id="b" style="width:200px;height:50px;background:#4a4">button</div>
<a id="a" href="about:newtab" style="display:block;height:50px">link</a>
<script>
var n=0; document.getElementById("b").addEventListener("click",function(e){n++;window.last=e.clientX+","+e.clientY});
document.getElementById("a").addEventListener("click",function(e){e.preventDefault();window.prevented=true});
</script></body>' "
60 click 50 110
70 click 60 115
80 js n+' '+last
90 click 50 160
140 js String(window.prevented)
141 dump" "
80 js=\"2 60,30\"
140 js=\"true\"
141 url=\"data:"
fi

# ── Hit-testing in a scrolled page (page coords = client + scroll) ───────────
if selected scrolled-click; then
run scrolled-click '<body style="margin:0">
<div style="height:2000px"></div>
<a href="about:newtab" style="display:block;height:50px">bottom</a></body>' "
50 click 600 400
60 key End
70 dump
80 click 20 875
140 dump" "
70 scroll=0,1235
140 url=\"about:newtab\""
fi

# ── Keyboard layouts: logical key / text from the layout win over the code ──
# (AZERTY: the key at QWERTY's Q types "a"; Ctrl+W is the key labelled W.)
if selected layout; then
run layout '<body style="margin:0">
<input id="q" style="position:absolute;left:10px;top:10px;width:300px;height:30px">
<script>window.k=[];document.addEventListener("keydown",function(e){k.push(e.key+":"+e.code)});</script>
</body>' "
60 click 100 110
70 keyas KeyQ a a
80 type é!
90 js document.getElementById('q').value
91 js k.join(',')
100 key Control+KeyT
110 dump
120 keyas Control+KeyW z
130 dump
140 keyas Control+KeyZ w
150 dump" "
90 js=\"aé!\"
91 js=\"a:KeyQ,é:Unidentified,!:Unidentified\"
110 tabs=2
130 tabs=2
150 tabs=1"
fi

[ $fails = 0 ] && echo "all passed" || { echo "$fails scenario(s) failed"; exit 1; }
