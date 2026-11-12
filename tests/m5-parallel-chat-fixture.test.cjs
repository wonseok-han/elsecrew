const test = require('node:test');
const assert = require('node:assert/strict');
const { parallelChatFixture } = require('./m5-parallel-chat-fixture.cjs');

test('parallel paint source isolates four bounded histories with complete pagination', () => {
  const emitted = [];
  const fixture = parallelChatFixture({ projectId: 'project', createdAt: '2026-10-13T00:00:00Z' }, message => emitted.push(message));
  for (let round = 0; round < 3000; round++) fixture.emitRound();
  assert.equal(emitted.length, 12000);
  assert.equal(new Set(emitted.map(message => message.messageId)).size, 12000);
  assert.deepEqual(Object.values(fixture.counts()), [3000, 3000, 3000, 3000]);
  for (const chat of fixture.chats) {
    let cursor = 0;
    const messages = [];
    do {
      const page = fixture.detail({ chatId: chat.chatId, afterEventSequence: cursor, limit: 200 });
      messages.push(...page.messages); cursor = page.nextEventSequence;
    } while (cursor !== null);
    assert.equal(messages.length, 3000);
    messages.forEach((message, index) => {
      assert.equal(message.chatId, chat.chatId);
      assert.equal(message.worktreeId, chat.worktreeId);
      assert.equal(message.eventSequence, index + 1);
    });
  }
  assert.throws(() => fixture.emitRound(), /exhausted/);
  assert.throws(() => fixture.detail({ chatId: 'unknown' }), /invalid/);
  assert.throws(() => fixture.detail({ chatId: fixture.chats[0].chatId, limit: 201 }), /invalid/);
});
