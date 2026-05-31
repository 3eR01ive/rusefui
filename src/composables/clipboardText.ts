async function tauriWrite(text: string): Promise<boolean> {
  try {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
    return true;
  } catch {
    return false;
  }
}

async function tauriRead(): Promise<string | null> {
  try {
    const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
    return await readText();
  } catch {
    return null;
  }
}

function execCommandCopy(text: string): boolean {
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.setAttribute("readonly", "true");
  ta.style.position = "fixed";
  ta.style.left = "-9999px";
  ta.style.top = "0";
  document.body.appendChild(ta);
  ta.focus();
  ta.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  document.body.removeChild(ta);
  return ok;
}

/** Запись в системный буфер обмена (Tauri → navigator → execCommand). */
export async function writeClipboardText(text: string): Promise<void> {
  if (await tauriWrite(text)) return;

  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // fallback ниже
    }
  }

  if (execCommandCopy(text)) return;

  throw new Error("Не удалось записать в буфер обмена");
}

/** Чтение из системного буфера обмена. */
export async function readClipboardText(): Promise<string> {
  const fromTauri = await tauriRead();
  if (fromTauri !== null && fromTauri !== "") {
    return fromTauri;
  }

  if (typeof navigator !== "undefined" && navigator.clipboard?.readText) {
    try {
      const text = await navigator.clipboard.readText();
      if (text) return text;
    } catch {
      // fallback ниже
    }
  }

  if (fromTauri !== null) return fromTauri;
  return "";
}
