# Records assets/demo.gif:  vhs demo/demo.tape   (from the repo's root; needs vhs and zaag installed)
Output assets/demo.gif
Set Shell bash
Set FontSize 15
Set Width 1180
Set Height 640
Set Padding 0
Set Framerate 20
Set TypingSpeed 70ms
Env ZELLIJ_SOCKET_DIR "/tmp/zaag-demo-sock"
Env ZAAG_STATE "/tmp/zaag-demo/state"
Env ZAAG_MACHINES " "

Hide
Type "sh demo/setup.sh && clear" Enter
Sleep 5s
Show

Type "zaag attach webapp" Sleep 400ms Enter
Sleep 6s
Ctrl+O Sleep 300ms Type "g"
Sleep 1800ms
Down Sleep 300ms Down Sleep 300ms Down Sleep 300ms Down Sleep 700ms
Enter
Sleep 3500ms
Ctrl+O Sleep 300ms Type "g"
Sleep 1200ms
Type "wri" Sleep 900ms
Enter
Sleep 3500ms

Hide
Ctrl+O Type "d"
Sleep 500ms
Type "sh demo/setup.sh --stop" Enter
Sleep 2s
