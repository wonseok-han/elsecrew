import type { Lang } from "../i18n.tsx";
import { G5_DEFAULT_ROLE_DEFINITIONS_V3, type LegacyRoleIdV1, type RoleProfileV1 } from "./contracts/run-recovery-v1.ts";
import { insertWorker, newMemberId, uniqueRoleName, type TeamDraft } from "./team-draft.ts";

/**
 * Built-in roles the '역할 추가' menu offers (custom roles R2, Q8). The text
 * lives here only: the sidecar never knows a template, it receives an
 * ordinary name and responsibility. The four starting roles reuse the
 * locale-independent seed (G5_DEFAULT_ROLE_DEFINITIONS_V3, equal to Rust);
 * the four added ones follow its style — deliverable, how to check it, where
 * it is written down.
 */
export type RoleTemplateKey = "planning" | "design" | "coding" | "review" | "tests" | "security" | "docs" | "research";
export const ROLE_TEMPLATE_KEYS: readonly RoleTemplateKey[] = ["planning", "design", "coding", "review", "tests", "security", "docs", "research"];

interface TemplateText { name: string; responsibility: string }
interface RoleTemplate {
  /** A starting role: added back under its original ID when the team lacks it. */
  legacyId?: Exclude<LegacyRoleIdV1, "coordinator">;
  permission: RoleProfileV1["permission"];
  text: Record<Lang, TemplateText>;
}

const seed = (roleId: LegacyRoleIdV1): TemplateText => {
  const definition = G5_DEFAULT_ROLE_DEFINITIONS_V3.find(item => item.roleId === roleId)!;
  return { name: definition.name, responsibility: definition.responsibility };
};

const TEMPLATES: ReadonlyMap<RoleTemplateKey, RoleTemplate> = new Map<RoleTemplateKey, RoleTemplate>([
  ["planning", { legacyId: "planning", permission: "workspaceWrite", text: { ko: seed("planning"), en: seed("planning") } }],
  ["design", { legacyId: "design", permission: "workspaceWrite", text: { ko: seed("design"), en: seed("design") } }],
  ["coding", { legacyId: "coding", permission: "workspaceWrite", text: { ko: seed("coding"), en: seed("coding") } }],
  ["review", { legacyId: "review", permission: "workspaceWrite", text: { ko: seed("review"), en: seed("review") } }],
  ["tests", { permission: "workspaceWrite", text: {
    ko: { name: "테스트 작성", responsibility: "완료 기준을 자동으로 확인하는 테스트를 만듭니다. 프로젝트가 이미 쓰는 테스트 도구가 있으면 그것을 따르고, 없으면 가장 단순한 방법을 이유와 함께 고릅니다. 정상 동작뿐 아니라 빈 입력, 잘못된 입력, 경계 값도 시험합니다. 테스트를 직접 실행해 통과·실패를 확인하고, 실패하면 제품 코드를 고치지 않고 재현 순서와 함께 개발에게 맡길 수정 사항으로 보고합니다. 실행 방법은 README에 적고, 무엇을 시험했고 무엇을 시험하지 못했는지 docs/tests.md 문서로 남깁니다." },
    en: { name: "Test writing", responsibility: "Writes tests that check the completion criteria automatically. Follows the test tool the project already uses, or picks the simplest one and says why. Tests empty input, wrong input and edge values as well as normal use. Runs the tests to see them pass or fail; when one fails, does not fix product code but reports the steps to reproduce as a fix for development. Puts how to run the tests in the README and records what was and was not tested in docs/tests.md." },
  } }],
  ["security", { permission: "readOnly", text: {
    ko: { name: "보안 점검", responsibility: "입력 검증, 비밀번호나 키 같은 비밀 값의 취급, 권한 확인, 밖으로 보내는 데이터를 살펴봅니다. 문제마다 위치, 위험한 이유, 실제로 일어날 수 있는 일, 고치는 방법을 쉬운 말로 적고 심각도(높음·보통·낮음)를 붙입니다. 파일은 고치지 않고 보고만 합니다. 직접 확인한 것과 추측을 구분하고, 확인하지 못한 부분은 '확인 못 함'으로 분명히 표시합니다." },
    en: { name: "Security check", responsibility: "Looks at input validation, how secrets such as passwords and keys are handled, permission checks and data sent outside. For each problem, writes in plain words where it is, why it is risky, what could actually happen and how to fix it, with a severity (high, medium, low). Does not change files and only reports. Separates what was checked from what is guessed, and marks anything not checked as 'not checked'." },
  } }],
  ["docs", { permission: "workspaceWrite", text: {
    ko: { name: "문서 정리", responsibility: "사용자가 프로젝트를 이해하고 실행할 수 있도록 README와 docs/ 문서를 정리합니다. 설치 방법, 실행 방법, 주요 기능, 자주 생기는 문제를 실제로 따라 해 보고 맞는지 확인합니다. 전문용어는 줄이고 꼭 필요하면 쉬운 설명을 붙입니다. 코드는 바꾸지 않고 오래되었거나 사실과 다른 문서를 고칩니다. 보고에는 바꾼 문서 목록과 확인하지 못한 내용을 적습니다." },
    en: { name: "Documentation", responsibility: "Tidies the README and the docs/ folder so the user can understand and run the project. Follows the install steps, run steps, main features and common problems to check they are right. Avoids jargon and explains any term that is needed. Does not change code; fixes documents that are out of date or wrong. Reports the documents changed and anything not checked." },
  } }],
  ["research", { permission: "readOnly", text: {
    ko: { name: "자료 조사", responsibility: "만들기 전에 알아야 할 것을 조사합니다. 프로젝트의 코드와 문서, 이미 설치된 라이브러리를 읽고, 쓸 수 있는 방법을 2~3개 찾아 장단점을 표로 비교한 뒤 추천 하나와 그 이유를 적습니다. 인터넷 검색은 할 수 없으니 프로젝트 안에서 확인한 내용과 알고 있는 지식을 구분해 적고, 확인하지 못한 내용은 추측이라고 표시합니다. 파일은 고치지 않고 보고만 합니다." },
    en: { name: "Research", responsibility: "Finds out what needs to be known before building. Reads the project's code, documents and installed libraries, compares two or three possible approaches in a table of pros and cons, and recommends one with the reason. Cannot search the internet, so separates what was confirmed in the project from general knowledge and marks anything unconfirmed as a guess. Does not change files and only reports." },
  } }],
]);

export function templatePermission(key: RoleTemplateKey): RoleProfileV1["permission"] {
  return TEMPLATES.get(key)!.permission;
}

/** A starting role the team still has under its original ID ('하나 더' adds a copy). */
export function templateInTeam(draft: TeamDraft, key: RoleTemplateKey): boolean {
  const legacyId = TEMPLATES.get(key)!.legacyId;
  return legacyId !== undefined && draft.roles.some(role => role.roleId === legacyId);
}

/**
 * Adds a template's role at `index`. A starting role the team lacks comes
 * back under its original ID with the seed name (so its translated label,
 * the verification stage of `review` and its report file title return);
 * otherwise a new `member-*` ID and a name not yet used. `shownName` is the
 * name the screen shows for the template (translated for starting roles).
 */
export function insertTemplate(draft: TeamDraft, index: number, key: RoleTemplateKey, lang: Lang, shownName: string, taken: Iterable<string>): { draft: TeamDraft; roleId: string } {
  const template = TEMPLATES.get(key)!;
  const text = template.text[lang];
  const restore = template.legacyId !== undefined && !templateInTeam(draft, key);
  const roleId = restore ? template.legacyId! : newMemberId([...taken, ...draft.roles.map(role => role.roleId)]);
  const name = uniqueRoleName(draft, restore ? text.name : shownName);
  const next = insertWorker(draft, index, { roleId, name, responsibility: text.responsibility, permission: template.permission });
  return { draft: next, roleId };
}

export function templateText(key: RoleTemplateKey, lang: Lang): TemplateText {
  return TEMPLATES.get(key)!.text[lang];
}
