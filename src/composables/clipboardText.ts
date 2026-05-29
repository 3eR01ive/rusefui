import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export async function writeClipboardText(text: string): Promise<void> {
  try {
    await writeText(text);
    return;
  } catch {
    // fallback ниже
  }
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // fallback ниже
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "true");
  ta.style.position = "fixed";
  ta.style.left = "-9999px";
  document.body.appendChild(ta);
  ta.select();
  document.execCommand("copy");
  document.body.removeChild(ta);
}

export async function readClipboardText(): Promise<string> {
  try {
    return await readText();
  } catch {
    // fallback ниже
  }
  try {
    return await navigator.clipboard.readText();
  } catch {
    return "";
  }
}
