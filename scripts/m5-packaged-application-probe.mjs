import assert from 'node:assert/strict';

// Only called for the runner's freshly generated project, after restart.
export async function probePackagedApplication(evaluate, until, readSource) {
  const button = text => `[...document.querySelectorAll('button')].find(item => item.textContent === ${JSON.stringify(text)})`;
  const click = async text => {
    await until(() => evaluate(`Boolean(${button(text)} && !${button(text)}.disabled)`));
    await evaluate(`${button(text)}.click()`);
  };
  await click('이 채팅의 반영 전 변경 목록 보기');
  await click('이 변경 묶음으로 확인 준비');
  await click('확인 취소');
  await until(() => evaluate(`!${button('확인 취소')}`));
  assert.equal(readSource(), 'original\n', 'Canceled review changed source');
  await click('이 변경 묶음으로 확인 준비');
  await until(() => evaluate(`Boolean(${button('백업 후 원본에 반영')})`));
  assert.equal(await evaluate(`${button('백업 후 원본에 반영')}.disabled`), true, 'Apply enabled without consent');
  const consent = `(() => {
    const label = [...document.querySelectorAll('label')].find(item => item.textContent.includes('원본 파일을 변경하는 작업') && item.querySelector('input') && !item.querySelector('input').checked);
    if (!label) throw new Error('Explicit consent control missing');
    label.querySelector('input').click();
  })()`;
  await evaluate(consent);
  await click('백업 후 원본에 반영');
  await until(() => evaluate(`document.body.innerText.includes('기록된 변경 묶음의 반영이 완료됐어요.')`));
  assert.equal(readSource(), 'edited in packaged snapshot\noriginal\n', 'Reviewed change was not applied exactly');
  await evaluate(`[...document.querySelectorAll('button')].filter(item => item.textContent === '반영·복원 기록 확인' && !item.disabled).forEach(item => item.click())`);
  await click('복구 기록의 파일 목록 확인');
  await until(() => evaluate(`Boolean(${button('기록을 확인하고 복원 시작')})`));
  assert.equal(await evaluate(`${button('기록을 확인하고 복원 시작')}.disabled`), true, 'Restore enabled without separate consent');
  await evaluate(consent);
  await click('기록을 확인하고 복원 시작');
  await until(() => evaluate(`document.body.innerText.includes('기록된 변경 묶음의 복원이 완료됐어요.')`));
  assert.equal(readSource(), 'original\n', 'Restoration did not restore original bytes');
  return { canceledReviewPreservedSource: true, applyRequiredConsent: true,
    appliedExactBytes: true, restoreRequiredSeparateConsent: true, restoredExactBytes: true };
}
