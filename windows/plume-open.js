// plume-open.js - launch plume flash-free, preferring Windows Terminal.
// Called by the Explorer context menus and shortcuts as
//   wscript plume-open.js <path>|-r
// Running under wscript.exe means no console window flashes on the way in.

var sh = new ActiveXObject("WScript.Shell");
var fso = new ActiveXObject("Scripting.FileSystemObject");

var here = fso.GetParentFolderName(WScript.ScriptFullName);
var exe = fso.BuildPath(here, "plume.exe");

function quote(s) { return '"' + s + '"'; }

// Turn the incoming path into plume arguments. A file roots plume at its parent
// folder so the tree shows its siblings, and a folder opens directly.
var arg = WScript.Arguments.length > 0 ? WScript.Arguments(0) : "-r";
var plumeArgs;
if (arg === "-r" || arg === "") {
    plumeArgs = "-r";
} else if (fso.FileExists(arg)) {
    plumeArgs = quote(fso.GetParentFolderName(arg)) + " " + quote(arg);
} else {
    plumeArgs = quote(arg);
}

// Prefer Windows Terminal when it is installed, otherwise the console host.
var wt = fso.BuildPath(sh.ExpandEnvironmentStrings("%LOCALAPPDATA%"),
                       "Microsoft\\WindowsApps\\wt.exe");
var command;
if (fso.FileExists(wt)) {
    command = quote(wt) + " " + quote(exe) + " " + plumeArgs;
} else {
    command = quote(exe) + " " + plumeArgs;
}

sh.Run(command, 1, false);
