import type { KeyboardEvent } from "react";

/** Enter sends and Shift+Enter inserts a newline, like common chat apps.
 * Command/Ctrl+Enter also sends. Keys pressed while an input method (e.g.
 * Korean) is still composing, held repeats and Alt combinations never send.
 * Sending goes through the same enabled button as a pointer click. */
export function submitChatShortcut(event: KeyboardEvent<HTMLFormElement>) {
  if (event.key !== "Enter" || event.shiftKey || event.altKey
    || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 || event.repeat) return;
  const target = event.target as HTMLElement;
  if (target.tagName !== "TEXTAREA" && target.tagName !== "INPUT") return;
  event.preventDefault();
  const submit = event.currentTarget.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (submit && !submit.disabled) submit.click();
}
