import assert from 'node:assert/strict';

// Uses Chromium input dispatch, not a mocked bridge or Monaco model mutation.
// One app session, not ten independent cold launches or physical keyboard/IME.
export async function probePackagedInput(evaluate, insertText) {
  const samples = [];
  for (let index = 0; index <= 10; index++) {
    const marker = `m5input${index}x`;
    await evaluate(`(() => {
      const input = document.querySelector('.monaco-editor textarea');
      if (!input) throw new Error('Large editor input missing');
      input.focus();
      if (document.activeElement !== input) throw new Error('Editor input not active');
      const sample = {started: performance.now(), received: null};
      globalThis.__m5PackagedInput = sample;
      const capture = event => {
        if (event.target === input && event.isTrusted && event.data === ${JSON.stringify(marker)}) {
          sample.received = performance.now();
          input.removeEventListener('beforeinput', capture, true);
        }
      };
      input.addEventListener('beforeinput', capture, true);
      sample.cleanup = () => input.removeEventListener('beforeinput', capture, true);
    })()`);
    try {
      await insertText(marker);
      const result = await evaluate(`(async () => {
        const sample = globalThis.__m5PackagedInput;
        const deadline = performance.now() + 5000;
        while (sample.received === null || ![...document.querySelectorAll('.monaco-editor .view-line')]
          .some(line => line.textContent.includes(${JSON.stringify(marker)}))) {
          if (performance.now() >= deadline) throw new Error('Trusted editor input or rendered text missing');
          await new Promise(resolve => setTimeout(resolve, 5));
        }
        const visible = performance.now();
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const painted = performance.now();
        return {inputPaintMs: painted - sample.started,
          inputDeliveryMs: sample.received - sample.started,
          inputAfterDeliveryMs: painted - sample.received,
          inputDomMs: visible - sample.started,
          paintObservationMs: painted - visible};
      })()`);
      assert.ok(Object.values(result).every(value => Number.isFinite(value) && value >= 0));
      samples.push({index, warmup:index === 0, ...result});
    } finally {
      await evaluate(`(() => { globalThis.__m5PackagedInput?.cleanup(); delete globalThis.__m5PackagedInput; })()`);
    }
  }
  const p95 = Math.max(...samples.slice(1).map(sample => sample.inputPaintMs));
  return {diagnostic:true, gateEligible:false, samples, p95, limitMs:50,
    withinLimit:p95 <= 50,
    limitations:['CDP text insertion, not physical keyboard or IME.',
      'One warmup plus ten edits in one app session; includes dispatch/observation overhead.',
      'Paint approximated by visible text followed by two animation frames; draft not saved.']};
}
