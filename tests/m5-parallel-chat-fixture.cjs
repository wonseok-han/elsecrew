// Synthetic source records, never a native persistence or paint observation.
function parallelChatFixture(template, publish) {
  const chats = Array.from({ length: 4 }, (_, index) => ({ ...template,
    chatId: `paint_chat_${index}`, taskId: `paint_task_${index}`,
    worktreeId: `paint_worktree_${index}`, workspaceIdentity: `paint_workspace_${index}`,
    goal: `Paint stream ${index + 1}`,
  }));
  const records = new Map(chats.map(chat => [chat.chatId, []]));
  return {
    chats,
    emitRound() {
      const emitted = [];
      for (const chat of chats) {
        const messages = records.get(chat.chatId);
        if (messages.length >= 3000) throw new Error('parallel fixture workload exhausted');
        const eventSequence = messages.length + 1;
        const message = { schemaVersion: 1, messageId: `${chat.chatId}_${eventSequence}`,
          chatId: chat.chatId, taskId: chat.taskId, worktreeId: chat.worktreeId,
          eventSequence, kind: 'user', roleId: null, runId: null,
          text: `${chat.chatId} message ${eventSequence}`, createdAt: template.createdAt };
        messages.push(message); publish(message); emitted.push(message);
      }
      return emitted;
    },
    detail({ chatId, afterEventSequence = 0, limit = 200 }) {
      const chat = chats.find(chat => chat.chatId === chatId);
      if (!chat || !Number.isSafeInteger(afterEventSequence) || afterEventSequence < 0 || !Number.isSafeInteger(limit) || limit < 1 || limit > 200) throw new Error('invalid parallel fixture page');
      const recordsForChat = records.get(chatId);
      const messages = recordsForChat.slice(afterEventSequence, afterEventSequence + limit);
      const last = afterEventSequence + messages.length;
      return { chat, afterEventSequence, messages, runs: [], nextEventSequence: last < recordsForChat.length ? last : null };
    },
    counts() { return Object.fromEntries([...records].map(([id, messages]) => [id, messages.length])); },
  };
}
module.exports = { parallelChatFixture };
