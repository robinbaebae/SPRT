# SPRT

**Claude Code 세션 사용량을 실시간으로 모니터링하는 macOS 메뉴바 앱**

<p align="center">
  <img src="src/assets/logo-black.png" alt="SPRT Logo" width="120" />
</p>

## 왜 만들었나?

Claude Code를 쓰다 보면 **"지금 사용량이 얼마나 남았지?"** 라는 생각이 자주 듭니다.
Rate limit에 걸려야 비로소 알게 되고, 세션 패턴을 돌아보기도 어렵습니다.

SPRT는 Claude Code의 로컬 데이터(`~/.claude/`)를 파싱하고 OAuth API를 통해 실시간 사용량을 메뉴바에서 바로 확인할 수 있게 해줍니다.

## 주요 기능

| 기능 | 설명 |
|------|------|
| **메뉴바 트레이** | 현재 5h 세션 사용률을 % 로 실시간 표시 |
| **팝오버** | 트레이 클릭 시 현재 세션 상태 즉시 확인 |
| **대시보드** | Rate Limits (5h/7d), 주간 활동 차트, 알림 |
| **실시간 갱신** | 파일 워처로 `~/.claude/` 변경 감지, 자동 새로고침 |
| **알림** | 80% 경고, 100% Rate Limit 도달 시 macOS 알림 |
| **라이트/다크** | 시스템 설정에 따라 자동 전환 |

## 기술 스택

- **Frontend**: React 19 + TypeScript + Vite 7
- **Backend**: Rust (Tauri v2)
- **데이터**: Claude Code 로컬 파일 (`stats-cache.json`, JSONL 세션 로그) + OAuth API
- **빌드**: Tauri Bundler → macOS DMG

## 아키텍처

```
┌─ macOS Menu Bar ──────────────────────┐
│  트레이 아이콘 + 사용률 %              │
│  ├─ 좌클릭 → Popover (세션 요약)      │
│  └─ 더블클릭 → Dashboard (전체 현황)   │
└───────────────────────────────────────┘

┌─ Rust Backend ────────────────────────┐
│  claude.rs  → stats-cache + JSONL 파싱 │
│  lib.rs     → 트레이, 윈도우, 파일워처  │
│  devlog.rs  → AI 개발 로그 생성        │
│  git.rs     → git 활동 파싱            │
└───────────────────────────────────────┘
```

## 설치

### 다운로드

[SPRT v0.2.0 DMG 다운로드](https://github.com/robinbaebae/SPRT/releases/latest)

### macOS 보안 설정

서명되지 않은 앱이므로 첫 실행 시 아래 명령어가 필요합니다:

```bash
xattr -cr /Applications/SPRT.app
```

또는 **시스템 설정 → 개인 정보 보호 및 보안 → 확인 없이 열기**

### 요구사항

- macOS 13.0 이상
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) 설치 및 로그인 완료

## 개발

```bash
# 의존성 설치
npm install

# 개발 서버 (Tauri + Vite HMR)
npm run tauri dev

# 프로덕션 빌드 (DMG)
npm run tauri build
```

## 디자인

- **글래스모피즘** 기반 UI (`backdrop-filter: blur`)
- **Sora** 폰트 (Google Fonts)
- 라이트/다크 모드 자동 전환

## 라이선스

MIT
