import pkg from "../package.json";








declare const __APP_VERSION__: string;




const RUNTIME_PACKAGE_VERSION = (pkg as { version: string }).version;









export const APP_VERSION: string =
  typeof __APP_VERSION__ !== "undefined" ? __APP_VERSION__ : RUNTIME_PACKAGE_VERSION;


// Single source of truth for the user-visible product name. Title bar,
// splash screen, About modal, and Settings hero all import this. CI
// patches the literal on canary builds (-> "ReChimera Canary") so every
// brand-name surface flips channel together, with no per-component
// patches needed.
export const APP_BRAND_NAME = "ReChimera";


export const APP_REPO_URL = "https://github.com/Mrroboto9819/ReChimera";



export const APP_ISSUES_URL = `${APP_REPO_URL}/issues`;













export async function openExternal(url: string): Promise<void> {
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}
