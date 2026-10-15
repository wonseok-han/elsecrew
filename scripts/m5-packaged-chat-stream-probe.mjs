// Serialized into the packaged renderer. Uses only the public production bridge.
// These are user-message events, not model tokens or agent.publicEvent records.
export function startPackagedChatStream(projectId) {
  if (globalThis.__m5NativeChatStream) throw new Error('Chat stream probe already started');
  const result = {diagnostic:true, gateEligible:false, status:'RUNNING',
    source:'public appendChatMessage', chats:[], sent:0, received:0,
    duplicateEvents:0, crossChatEvents:0, reorderedEvents:0,
    maximumDeliveryMs:0, maximumDispatchLagMs:0, paintSamples:[],
    limitations:['User-message IPC and persistence workload; no provider or model calls.',
      'Only the selected first chat is paint-observed; two animation frames are not physical compositor timestamps.',
      'No child-reaping stress, repeated ten-run result or complete G6 evidence.']};
  globalThis.__m5NativeChatStream = result;
  void (async () => {
    const bridge = window.m5;
    const expected = new Map(), received = new Set(), sequences = new Map(), initialMessages = new Map();
    const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
    const check = (condition, detail) => { if (!condition) throw new Error(detail); };
    const wait = async ready => {
      const deadline = performance.now() + 65000;
      while (!ready()) {
        check(performance.now() < deadline, 'Chat UI did not become ready');
        await sleep(25);
      }
    };
    const select = async goal => {
      const button = [...document.querySelectorAll('.m5-chat-chip')].find(node => node.textContent.includes(goal) && !node.disabled);
      check(button, 'Chat selection missing'); button.click();
      await wait(() => document.querySelector('.m5-timeline h2')?.textContent === goal);
    };
    const paintPending = new Map(), frames = new Set();
    const frame = callback => {
      const id = requestAnimationFrame(() => { frames.delete(id); callback(); });
      frames.add(id);
    };
    let unsubscribe, observer, stopped = false;
    try {
      for (let index = 0; index < 4; index++) {
        const goal = `Native stream ${index + 1}`;
        await wait(() => document.querySelector('.m5-new-chat-button') && !document.querySelector('.m5-new-chat-button').disabled);
        document.querySelector('.m5-new-chat-button').click();
        await wait(() => document.querySelector('.m5-new-chat-dialog')?.matches(':modal'));
        const input = document.querySelector('.m5-new-chat-form input:not([type=checkbox])');
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, goal);
        input.dispatchEvent(new Event('input', {bubbles:true}));
        await wait(() => !document.querySelector('.m5-new-chat-form button[type=submit]').disabled);
        document.querySelector('.m5-new-chat-form button[type=submit]').click();
        await wait(() => document.querySelector('.m5-timeline h2')?.textContent === goal);
        const catalog = await bridge.listChats({projectId, cursor:null, limit:100});
        const matches = catalog.items.filter(chat => chat.goal === goal);
        check(matches.length === 1, 'Created chat identity is ambiguous');
        const chat = matches[0];
        const detail = await bridge.getChat({chatId:chat.chatId, afterEventSequence:0, cursor:null, limit:50});
        check(detail.messages.length === 1 && detail.messages[0].eventSequence === 1 && detail.nextEventSequence === null,
          'New chat must have exactly its initial goal');
        initialMessages.set(detail.messages[0].messageId, detail.messages[0]);
        result.chats.push({...chat, sent:0, received:0, persisted:0});
        sequences.set(chat.chatId, 1);
      }
      check(new Set(result.chats.map(chat => chat.chatId)).size === 4 &&
        new Set(result.chats.map(chat => chat.workspaceIdentity)).size === 4, 'Chats must be isolated');
      await select(result.chats[0].goal);
      const observePaint = () => {
        const visibleText = new Set([...document.querySelectorAll('.m5-message p')]
          .filter(node => node.getClientRects().length).map(node => node.textContent));
        for (const [id, source] of paintPending) {
          if (!visibleText.has(source.text)) continue;
          paintPending.delete(id);
          frame(() => frame(() => {
            if (stopped) return;
            if (document.visibilityState !== 'visible') { result.paintVisibilityFailed = true; return; }
            result.paintSamples.push({messageId:id, eventSequence:source.sequence,
              eventPaintMs:performance.now() - source.receivedAt,
              dispatchPaintMs:performance.now() - source.dispatchedAt});
          }));
        }
      };
      observer = new MutationObserver(observePaint);
      observer.observe(document.querySelector('.m5-timeline'), {childList:true, subtree:true, characterData:true});
      unsubscribe = bridge.onChatEvent(event => {
        // Creation responses need not imply their seq-1 events have drained.
        // Only exact identities read back from these four new chats are exempt
        // from the subsequent 12,000-message workload, never all seq-1 events.
        const initial = initialMessages.get(event.messageId);
        if (initial) {
          if (['chatId', 'taskId', 'worktreeId', 'text', 'eventSequence', 'kind'].some(key => event[key] !== initial[key])) result.crossChatEvents++;
          return;
        }
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
        if (source.chat.chatId === result.chats[0].chatId) {
          paintPending.set(event.messageId, {...source, receivedAt:performance.now()});
          observePaint();
        }
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
      const paintDeadline = performance.now() + 10000;
      while (result.paintSamples.length < 3000 && performance.now() < paintDeadline) await sleep(25);
      check(!result.paintVisibilityFailed && result.paintSamples.length === 3000, 'Active chat paint observations incomplete');
      result.maximumEventPaintMs = Math.max(...result.paintSamples.map(sample => sample.eventPaintMs));
      result.eventPaintWithinLimit = result.maximumEventPaintMs <= 100;
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
      result.restoredChats = [];
      for (const [index, chat] of result.chats.entries()) {
        await select(chat.goal);
        await wait(() => {
          const texts = [...document.querySelectorAll('.m5-message p')].map(node => node.textContent);
          return texts.length === 200 && texts.every((text, offset) => text === `native_stream_${index}_${2801 + offset}`);
        });
        result.restoredChats.push(chat.chatId);
      }
      check(result.eventPaintWithinLimit, 'Active chat event paint exceeds 100ms');
      result.status = 'NOT_READY'; // Real IPC evidence, but not a full performance gate.
    } catch (error) { result.status = 'FAIL'; result.error = String(error); }
    finally { stopped = true; unsubscribe?.(); observer?.disconnect(); for (const id of frames) cancelAnimationFrame(id); result.finishedAt = performance.now(); }
  })();
  return {started:true};
}
