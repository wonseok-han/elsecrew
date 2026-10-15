// Serialized into the packaged renderer. Uses only the public production bridge.
// These are user-message events, not model tokens or agent.publicEvent records.
export function startPackagedChatStream(projectId) {
  if (globalThis.__m5NativeChatStream) throw new Error('Chat stream probe already started');
  const result = {diagnostic:true, gateEligible:false, status:'RUNNING',
    source:'public appendChatMessage', chats:[], sent:0, received:0,
    duplicateEvents:0, crossChatEvents:0, reorderedEvents:0,
    maximumDeliveryMs:0, maximumDispatchLagMs:0,
    limitations:['User-message IPC and persistence workload; no provider or model calls.',
      'Delivery is not paint. No active-chat presentation, child-reaping stress or complete G6 evidence.']};
  globalThis.__m5NativeChatStream = result;
  void (async () => {
    const bridge = window.m5;
    const expected = new Map(), received = new Set(), sequences = new Map();
    const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
    const check = (condition, detail) => { if (!condition) throw new Error(detail); };
    let unsubscribe, stopped = false;
    try {
      const settings = await bridge.getProjectRoleSettings({projectId});
      for (let index = 0; index < 4; index++) {
        const prepared = await bridge.prepareChatWorkspace({projectId});
        check(prepared.bindingState === 'unbound' && prepared.manifest.projectId === projectId, 'Prepared chat identity differs');
        const {worktreeId, workspaceIdentity} = prepared.manifest;
        const chat = await bridge.createChat({projectId, worktreeId, workspaceIdentity,
          preparationId:prepared.preparationId, goal:`Native stream ${index + 1}`,
          expectedSettingsRevision:settings.revision});
        const detail = await bridge.getChat({chatId:chat.chatId, afterEventSequence:0, cursor:null, limit:50});
        check(detail.messages.length === 1 && detail.messages[0].eventSequence === 1 && detail.nextEventSequence === null,
          'New chat must have exactly its initial goal');
        result.chats.push({...chat, sent:0, received:0, persisted:0});
        sequences.set(chat.chatId, 1);
      }
      check(new Set(result.chats.map(chat => chat.chatId)).size === 4 &&
        new Set(result.chats.map(chat => chat.workspaceIdentity)).size === 4, 'Chats must be isolated');
      unsubscribe = bridge.onChatEvent(event => {
        const source = expected.get(event.messageId);
        if (!source) { result.crossChatEvents++; return; }
        if (received.has(event.messageId)) { result.duplicateEvents++; return; }
        received.add(event.messageId);
        if (event.chatId !== source.chat.chatId || event.taskId !== source.chat.taskId ||
            event.worktreeId !== source.chat.worktreeId || event.text !== source.text) result.crossChatEvents++;
        if (event.eventSequence !== sequences.get(source.chat.chatId) + 1) result.reorderedEvents++;
        sequences.set(source.chat.chatId, event.eventSequence);
        source.chat.received++; result.received++;
        result.maximumDeliveryMs = Math.max(result.maximumDeliveryMs, performance.now() - source.dispatchedAt);
      });
      const startedAt = performance.now();
      result.startedAt = startedAt;
      // Sequential within each chat preserves the public optimistic sequence contract.
      // Four chains overlap; slow dispatch is measured, never masked by retries.
      const outcomes = await Promise.allSettled(result.chats.map(async (chat, chatIndex) => {
        try {
          for (let index = 0; index < 3000; index++) {
            if (stopped) break;
            const due = startedAt + index * 20;
            await sleep(Math.max(0, due - performance.now()));
            if (stopped) break;
            const clientMessageId = `user_${crypto.randomUUID().replaceAll('-', '')}`;
            const text = `native_stream_${chatIndex}_${index + 1}`;
            const dispatchedAt = performance.now();
            result.maximumDispatchLagMs = Math.max(result.maximumDispatchLagMs, dispatchedAt - due);
            expected.set(clientMessageId, {chat, text, dispatchedAt, sequence:index + 2});
            const reply = await bridge.appendChatMessage({projectId, chatId:chat.chatId, taskId:chat.taskId,
              worktreeId:chat.worktreeId, workspaceIdentity:chat.workspaceIdentity, clientMessageId,
              expectedEventSequence:index + 1, text});
            check(reply.messageId === clientMessageId && reply.chatId === chat.chatId && reply.text === text &&
              reply.eventSequence === index + 2, 'Append response differs');
            chat.sent++; result.sent++;
          }
        } catch (error) { stopped = true; throw error; }
      }));
      result.dispatchDurationMs = performance.now() - startedAt;
      const failure = outcomes.find(outcome => outcome.status === 'rejected');
      if (failure) throw failure.reason;
      const deadline = performance.now() + 10000;
      while (result.received < result.sent && performance.now() < deadline) await sleep(25);
      result.lostEvents = result.sent - received.size;
      check(result.sent === 12000 && result.lostEvents === 0 && result.duplicateEvents === 0 &&
        result.crossChatEvents === 0 && result.reorderedEvents === 0, 'Stream delivery differs');
      for (const chat of result.chats) {
        let after = 1;
        for (let pageIndex = 0; pageIndex < 61; pageIndex++) {
          const page = await bridge.getChat({chatId:chat.chatId, afterEventSequence:after, cursor:null, limit:50});
          check(page.chat.chatId === chat.chatId && page.messages.length <= 50, 'History page identity differs');
          for (const message of page.messages) {
            const source = expected.get(message.messageId);
            check(source?.chat.chatId === chat.chatId && source.text === message.text &&
              source.sequence === message.eventSequence && message.eventSequence === after + 1, 'Persisted history differs');
            after = message.eventSequence; chat.persisted++;
          }
          if (page.nextEventSequence === null) break;
          check(page.messages.length > 0 && page.nextEventSequence === after, 'History cursor did not advance');
        }
        check(chat.persisted === 3000, 'Persisted message count differs');
      }
      result.status = 'NOT_READY'; // Real IPC evidence, but not a full performance gate.
    } catch (error) { result.status = 'FAIL'; result.error = String(error); }
    finally { stopped = true; unsubscribe?.(); result.finishedAt = performance.now(); }
  })();
  return {started:true};
}
