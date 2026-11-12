import assert from 'node:assert/strict';

// Aggregate only complete raw trials; never substitute a supplied PASS flag.
export function packagedInputMaximum(input) {
  assert.ok(input?.diagnostic === true && input.gateEligible === false);
  assert.equal(input.limitMs, 50);
  assert.equal(input.samples?.length, 11);
  for (const [index, sample] of input.samples.entries()) {
    assert.equal(sample.index, index);
    assert.equal(sample.warmup, index === 0);
    for (const key of ['inputPaintMs', 'inputDeliveryMs', 'inputAfterDeliveryMs', 'inputDomMs', 'paintObservationMs']) {
      assert.ok(Number.isFinite(sample[key]) && sample[key] >= 0, `Invalid ${key}`);
    }
  }
  const maximum = Math.max(...input.samples.slice(1).map(sample => sample.inputPaintMs));
  assert.equal(input.p95, maximum);
  assert.equal(input.withinLimit, maximum <= 50);
  return maximum;
}

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
      let observer;
      let deadline;
      let firstFrame;
      let secondFrame;
      let observingPaint = false;
      sample.result = new Promise((resolve, reject) => {
        const check = () => {
          if (observingPaint || sample.received === null || ![...document.querySelectorAll('.monaco-editor .view-line')]
            .some(line => line.textContent.includes(${JSON.stringify(marker)}))) return;
          observingPaint = true;
          observer.disconnect();
          const visible = performance.now();
          firstFrame = requestAnimationFrame(() => {
            secondFrame = requestAnimationFrame(() => {
              clearTimeout(deadline);
              const painted = performance.now();
              resolve({inputPaintMs: painted - sample.started,
                inputDeliveryMs: sample.received - sample.started,
                inputAfterDeliveryMs: painted - sample.received,
                inputDomMs: visible - sample.started,
                paintObservationMs: painted - visible});
            });
          });
        };
        observer = new MutationObserver(check);
        observer.observe(input.closest('.monaco-editor'), {subtree:true, childList:true, characterData:true});
        sample.check = check;
        deadline = setTimeout(() => reject(new Error('Trusted editor input or rendered text missing')), 5000);
      });
      // The CDP command may fail before the host starts awaiting this promise.
      sample.result.catch(() => {});
      const capture = event => {
        if (event.target === input && event.isTrusted && event.data === ${JSON.stringify(marker)}) {
          sample.received = performance.now();
          input.removeEventListener('beforeinput', capture, true);
          queueMicrotask(sample.check);
        }
      };
      input.addEventListener('beforeinput', capture, true);
      sample.cleanup = () => {
        input.removeEventListener('beforeinput', capture, true);
        observer.disconnect();
        clearTimeout(deadline);
        cancelAnimationFrame(firstFrame);
        cancelAnimationFrame(secondFrame);
      };
    })()`);
    try {
      await insertText(marker);
      const result = await evaluate('globalThis.__m5PackagedInput.result');
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
      'One warmup plus ten edits in one app session; observer installed before dispatch, includes dispatch/observation overhead.',
      'Paint approximated by visible text followed by two animation frames; draft not saved.']};
}
