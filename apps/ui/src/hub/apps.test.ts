import { expect, it } from "vitest";
import { appDisplay } from "./apps";
it.each([
  ["Code.exe", "editor", "VS Code", "Code"], ["Cursor.exe", "editor", "Cursor", "Code"],
  ["WindowsTerminal.exe", "terminal", "Terminal", "TerminalWindow"], ["wt.exe", "terminal", "Terminal", "TerminalWindow"],
  ["pwsh.exe", "terminal", "PowerShell", "TerminalWindow"], ["powershell.exe", "terminal", "PowerShell", "TerminalWindow"],
  ["cmd.exe", "terminal", "Command Prompt", "TerminalWindow"], ["slack.exe", "prose", "Slack", "ChatCircle"],
  ["Discord.exe", "prose", "Discord", "ChatCircle"], ["Notion.exe", "prose", "Notion", "Note"],
  ["chrome.exe", "prose", "Chrome", "Browser"], ["msedge.exe", "prose", "Edge", "Browser"], ["firefox.exe", "prose", "Firefox", "Browser"],
  ["olk.exe", "prose", "Outlook", "EnvelopeSimple"], ["OUTLOOK.EXE", "prose", "Outlook", "EnvelopeSimple"],
  ["notepad.exe", "default", "Notepad", "AppWindow"], ["explorer.exe", "default", "File Explorer", "FolderSimple"],
])("%s", (exe, profile, name, icon) => expect(appDisplay(exe, profile)).toMatchObject({ name, icon }));
it("unknown exe: name without .exe, icon by profile, colour by profile", () => {
  expect(appDisplay("zed.exe", "editor")).toEqual({ name: "zed", icon: "Code", chip: "editor" });
  expect(appDisplay("alacritty.exe", "terminal")).toEqual({ name: "alacritty", icon: "TerminalWindow", chip: "terminal" });
  expect(appDisplay("Obsidian.exe", "prose")).toEqual({ name: "Obsidian", icon: "ChatCircle", chip: "prose" });
  expect(appDisplay("foo.exe", "weird")).toEqual({ name: "foo", icon: "AppWindow", chip: "default" });
  expect(appDisplay("", "default").name).toBe("Unknown app");
});
it("colour comes from the row's profile, not the exe", () => expect(appDisplay("Code.exe", "prose").chip).toBe("prose"));
