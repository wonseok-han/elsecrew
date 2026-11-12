import type { KeyboardEvent } from "react";

/** Route keyboard submission through the same enabled button as a pointer click. */
export function submitChatShortcut(event: KeyboardEvent<HTMLFormElement>) {
  if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey
    || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 || event.repeat) return;
  const target = event.target as HTMLElement;
  if (target.tagName !== "TEXTAREA" && target.tagName !== "INPUT") return;
  event.preventDefault();
  const submit = event.currentTarget.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (submit && !submit.disabled) submit.click();
}
