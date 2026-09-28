import type { LoginProvider } from "./contracts/provider-login-v1.ts";

export function loginErrorMessage(provider: LoginProvider, error: string | null): string {
  if (error === "providerLoginRecoveryRequired") return "로그인 프로세스 종료를 확인하지 못했어요. 새 로그인과 안전 종료를 차단하고 복구 기록을 보존했어요. 인증 파일을 지우거나 강제 종료하지 말고 아래 정보로 도움을 요청해 주세요.";
  if (error === "providerExecutableChanged") return "설치된 에이전트가 변경되어 이 앱에서 호환성을 확인할 수 없어요. 로그아웃된 것은 아니며 저장된 로그인 정보는 지우지 않았어요. 앱의 호환성 업데이트가 필요해요.";
  if (error === "providerExecutableMissing") return "지원하는 에이전트 실행 파일을 찾지 못했어요. 에이전트 설치 상태를 확인해 주세요. 저장된 로그인 정보는 지우지 않았어요.";
  if (error === "providerProfileUnavailable") return "저장된 로그인 환경을 안전하게 열지 못했어요. 로그아웃 여부는 아직 확인하지 못했어요. 인증 파일을 삭제하지 말고 아래 정보로 도움을 요청해 주세요.";
  return provider === "claude"
    ? "Claude 연결을 확인하지 못했어요. 코드가 만료됐다면 새로 로그인을 시작해 새 코드를 입력해 주세요. 키체인 오류가 나타나면 기본값으로 재설정하지 말고 취소해 주세요."
    : "저장된 Codex 연결을 확인하지 못했어요. 이 메시지만으로 로그아웃된 것은 아니에요. 인증 파일을 삭제하지 말고 아래 정보로 도움을 요청해 주세요.";
}
