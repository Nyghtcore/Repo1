/**
 * Platform helpers. The same React bundle ships in the Windows desktop app
 * and the Android companion; a few features (native folder picker, "reveal in
 * Explorer", path-based file import) only exist on desktop.
 */

export const isMobile: boolean =
  typeof navigator !== "undefined" && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);

export const isAndroid: boolean =
  typeof navigator !== "undefined" && /Android/i.test(navigator.userAgent);

/** Read a browser `File` as a base64 string (without the data-URL prefix). */
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("Could not read file"));
    reader.onload = () => {
      const result = String(reader.result ?? "");
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.readAsDataURL(file);
  });
}
