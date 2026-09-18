export type AppIcon = "Code" | "TerminalWindow" | "ChatCircle" | "Note" | "Browser" | "EnvelopeSimple" | "AppWindow" | "FolderSimple";
export type ChipProfile = "default" | "terminal" | "editor" | "prose";
const KNOWN: Record<string, { name: string; icon: AppIcon }> = {
  "code.exe": { name: "VS Code", icon: "Code" }, "cursor.exe": { name: "Cursor", icon: "Code" },
  "windowsterminal.exe": { name: "Terminal", icon: "TerminalWindow" }, "wt.exe": { name: "Terminal", icon: "TerminalWindow" },
  "powershell.exe": { name: "PowerShell", icon: "TerminalWindow" }, "pwsh.exe": { name: "PowerShell", icon: "TerminalWindow" },
  "cmd.exe": { name: "Command Prompt", icon: "TerminalWindow" },
  "slack.exe": { name: "Slack", icon: "ChatCircle" }, "discord.exe": { name: "Discord", icon: "ChatCircle" },
  "notion.exe": { name: "Notion", icon: "Note" },
  "chrome.exe": { name: "Chrome", icon: "Browser" }, "msedge.exe": { name: "Edge", icon: "Browser" }, "firefox.exe": { name: "Firefox", icon: "Browser" },
  "olk.exe": { name: "Outlook", icon: "EnvelopeSimple" }, "outlook.exe": { name: "Outlook", icon: "EnvelopeSimple" },
  "notepad.exe": { name: "Notepad", icon: "AppWindow" }, "explorer.exe": { name: "File Explorer", icon: "FolderSimple" },
};
const BY_PROFILE: Record<ChipProfile, AppIcon> = { editor: "Code", terminal: "TerminalWindow", prose: "ChatCircle", default: "AppWindow" };
export function chipProfile(profile: string): ChipProfile {
  return profile === "editor" || profile === "terminal" || profile === "prose" ? profile : "default";
}
export function appDisplay(exe: string, profile: string) {
  const chip = chipProfile(profile);
  const known = KNOWN[exe.toLowerCase()];
  if (known) return { ...known, chip };
  const name = exe.replace(/\.exe$/i, "") || "Unknown app";
  return { name, icon: BY_PROFILE[chip], chip };
}
