import type { Metadata } from "next";
import { headers } from "next/headers";
import "./globals.css";

export async function generateMetadata(): Promise<Metadata> {
  const requestHeaders = await headers();
  const host = requestHeaders.get("host") ?? "elsecrew.dev";
  const protocol = requestHeaders.get("x-forwarded-proto") ?? "https";
  const baseUrl = `${protocol}://${host}`;

  return {
    metadataBase: new URL(baseUrl),
    title: "Elsecrew — 터미널 없이 코딩 에이전트 시작하기",
    description:
      "Claude Code와 Codex의 설치부터 로그인, 프로젝트 관리와 업데이트 확인까지 돕는 초보자용 데스크톱 앱입니다.",
    openGraph: {
      title: "터미널을 몰라도, 코딩 에이전트는 쓸 수 있어요.",
      description:
        "설치부터 첫 대화, 프로젝트와 에이전트 관리까지 Elsecrew가 차근차근 안내합니다.",
      type: "website",
      locale: "ko_KR",
      siteName: "Elsecrew",
      images: [
        {
          url: `${baseUrl}/og.png`,
          width: 1731,
          height: 908,
          alt: "Elsecrew — 터미널 없이 시작하는 코딩 에이전트",
        },
      ],
    },
    twitter: {
      card: "summary_large_image",
      title: "Elsecrew",
      description: "터미널 없이 시작하는 나의 첫 코딩 에이전트",
      images: [`${baseUrl}/og.png`],
    },
  };
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="ko">
      <body>{children}</body>
    </html>
  );
}
