/** Which parts of `after` are new relative to `before`, for <mark> highlighting.
 *  Token LCS on lower-cased words and punctuation; a token that only changed case is
 *  marked letter by letter (unless ignoreCase). Deletions are not shown. */
export interface Seg { text: string; changed: boolean }
const TOKEN = /[\p{L}\p{N}_](?:[\p{L}\p{N}_'.:-]*[\p{L}\p{N}_])?|[^\s\p{L}\p{N}_]/gu;
interface Tok { text: string; start: number }
const tokens = (s: string): Tok[] => [...s.matchAll(TOKEN)].map((m) => ({ text: m[0], start: m.index! }));

export function markChanges(before: string, after: string, opts: { ignoreCase?: boolean; ignoreFinalPeriod?: boolean } = {}): Seg[] {
  const a = tokens(after), b = tokens(before);
  const al = a.map((t) => t.text.toLowerCase()), bl = b.map((t) => t.text.toLowerCase());
  // LCS table
  const L = Array.from({ length: a.length + 1 }, () => new Uint16Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i--) for (let j = b.length - 1; j >= 0; j--)
    L[i][j] = al[i] === bl[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
  const match = new Array<number>(a.length).fill(-1);
  for (let i = 0, j = 0; i < a.length && j < b.length; ) {
    if (al[i] === bl[j]) { match[i] = j; i++; j++; } else if (L[i + 1][j] >= L[i][j + 1]) i++; else j++;
  }
  const flags = new Uint8Array(after.length); // 1 = changed character
  a.forEach((t, i) => {
    const last = i === a.length - 1;
    if (match[i] < 0) {
      if (opts.ignoreFinalPeriod && last && t.text === ".") return;
      flags.fill(1, t.start, t.start + t.text.length);
    } else if (!opts.ignoreCase) {
      const src = b[match[i]].text;
      for (let k = 0; k < t.text.length; k++) if (t.text[k] !== src[k]) flags[t.start + k] = 1;
    }
  });
  const out: Seg[] = [];
  for (let i = 0; i < after.length; i++) {
    const changed = flags[i] === 1, prev = out[out.length - 1];
    if (prev && prev.changed === changed) prev.text += after[i];
    else out.push({ text: after[i], changed });
  }
  return out;
}
