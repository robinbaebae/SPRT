# SPRT — 작업 노트

> 이 문서만 있으면 SPRT 프로젝트를 이어서 수정할 수 있도록 정리한 작업 레퍼런스.

---

## 1. 프로젝트 개요

| 항목 | 내용 |
|------|------|
| **이름** | SPRT (Sprint) |
| **설명** | macOS 메뉴바 앱. Claude Code 세션 사용량을 실시간 모니터링 |
| **경로** | `/Users/sooyoungbae/claude-monitor` |
| **GitHub** | `https://github.com/robinbaebae/SPRT` |
| **버전** | 0.1.0 |
| **identifier** | `com.claudemonitor.dev` |

---

## 2. 기술 스택

### Frontend
- **React 19** + **TypeScript** + **Vite 7**
- 단일 컴포넌트 구조: `src/App.tsx` (467줄) + `src/App.css` (876줄)
- 폰트: Sora (Google Fonts)
- Tauri API: `@tauri-apps/api`, `@tauri-apps/plugin-notification`

### Backend (Rust / Tauri v2)
- **Tauri 2** (`src-tauri/`)
- Rust 모듈:
  - `claude.rs` (701줄) — Claude API stats-cache 파싱, rate limit 조회, 세션 감지
  - `devlog.rs` (471줄) — AI 기반 개발 로그 생성 (Claude API 호출)
  - `git.rs` (266줄) — git log/diff 파싱
  - `storage.rs` (132줄) — DevLog 저장/로드 (~/.claude/devlogs/)
  - `lib.rs` (233줄) — Tauri 앱 setup, 트레이, 윈도우 관리, 파일 감시
- 주요 Rust 의존성: `notify` (파일 워처), `reqwest` (HTTP), `chrono`, `dirs`, `image`

### 랜딩페이지
- `landing/index.html` (영문) — 단일 HTML, 인라인 CSS, 빌드 불필요
- `landing/ko.html` (한국어) — 동일 구조
- 배포: GitHub Pages 또는 정적 호스팅

---

## 3. 프로젝트 구조

```
claude-monitor/
├── src/
│   ├── App.tsx          # 메인 React (Popover + Dashboard 두 뷰)
│   ├── App.css          # 전체 스타일 (라이트/다크 모드)
│   ├── main.tsx         # React 엔트리
│   └── assets/
│       ├── logo-white.png
│       └── logo-black.png
├── src-tauri/
│   ├── tauri.conf.json  # 앱 설정 (윈도우, 번들, 아이콘)
│   ├── Cargo.toml       # Rust 의존성
│   ├── capabilities/
│   │   └── default.json # Tauri 권한 설정
│   ├── icons/           # 앱 아이콘, 트레이 아이콘
│   │   ├── tray-icon.png
│   │   └── tray-icon@2x.png
│   └── src/
│       ├── lib.rs       # Tauri setup (트레이, 윈도우, 파일워처)
│       ├── main.rs      # 엔트리
│       ├── claude.rs    # Claude 데이터 파싱 & rate limit
│       ├── devlog.rs    # 개발 로그 생성
│       ├── git.rs       # Git 활동 파싱
│       └── storage.rs   # DevLog 영속 저장
├── landing/
│   ├── index.html       # 영문 랜딩페이지
│   ├── ko.html          # 한국어 랜딩페이지
│   └── images/          # 스크린샷 (현재 미사용)
├── index.html           # Vite 엔트리 (Tauri webview용)
├── package.json
├── vite.config.ts
└── tsconfig.json
```

---

## 4. 아키텍처

### 윈도우 구성 (tauri.conf.json)
1. **main** — 대시보드 (312×420, alwaysOnTop, titleBarStyle: Overlay)
2. **popover** — 트레이 클릭 시 팝오버 (250×100, decorations: false, transparent)

### 앱 동작 방식
- `ActivationPolicy::Accessory` → Dock에 안 보이고 메뉴바만 표시
- 트레이 좌클릭 → popover 토글 (현재 세션 5h 사용률)
- 트레이 더블클릭 / 우클릭 "Open Dashboard" → main 윈도우
- main 윈도우 닫기 → 숨기기 (앱 종료 X)
- popover 포커스 잃으면 자동 숨기기
- 5초마다 트레이 타이틀 업데이트 (사용률 %)
- `notify` 크레이트로 `~/.claude/stats-cache.json` + `~/.claude/projects/` 감시 → 변경 시 `claude-data-changed` 이벤트 emit

### 데이터 소스
- `~/.claude/stats-cache.json` — 전체 통계 (일별 활동, 모델별 토큰)
- `~/.claude/projects/*/session.jsonl` — 개별 세션 로그
- `~/.claude/projects/*/rate_limits.jsonl` — rate limit 기록
- Claude API (reqwest) — devlog 생성 시 Claude 호출

### Tauri Commands (프론트→백엔드)
```rust
claude::get_stats_cache        // 전체 통계
claude::get_active_sessions    // 활성 세션 목록
claude::get_project_usage      // 프로젝트별 사용량
claude::get_realtime_stats     // 실시간 통계
claude::get_rate_limits        // rate limit 정보
devlog::generate_devlog        // AI 개발 로그 생성
devlog::get_devlog             // 특정 날짜 devlog 조회
devlog::list_devlogs           // devlog 목록
devlog::get_git_activity       // git 활동 조회
update_tray_title              // 트레이 타이틀 수동 변경
open_dashboard                 // 대시보드 열기
```

---

## 5. 작업 진행 이력

### Phase 1: 앱 개발
1. Tauri v2 + React + TypeScript 프로젝트 생성
2. Claude `~/.claude/` 데이터 파싱 시스템 구현 (Rust)
   - stats-cache.json 파싱
   - session.jsonl 파싱 (프로젝트별)
   - rate_limits.jsonl 파싱 (5h window, 7-day, sonnet 별도)
3. macOS 메뉴바 트레이 앱 구현
   - 트레이 아이콘 + 타이틀 (사용률 %)
   - 팝오버 (현재 세션 사용률 바)
   - 대시보드 (전체 통계, 차트, rate limits)
4. 대시보드 UI 구현
   - 헤더: 플랜 타입, 마지막 활동, 활성 세션 수
   - Rate Limits: 5h window, 7-day All Models, 7-day Sonnet (프로그레스 바)
   - 7-Day Activity 차트 (일별 메시지 수)
   - 알림: rate limit 80% 도달 시, 리셋 시 macOS 알림
5. DevLog 기능 구현
   - git 활동 + Claude 세션 데이터 → AI가 일일 개발 요약 생성
   - Sprint Score (0-100)
6. 파일 워처 → 데이터 변경 시 자동 새로고침
7. 라이트/다크 모드 지원 (CSS prefers-color-scheme)

### Phase 2: 빌드 & 배포
1. 앱 아이콘 + 트레이 아이콘 제작
2. `npm run tauri build` → DMG 생성
   - 출력: `src-tauri/target/release/bundle/dmg/SPRT_0.1.0_aarch64.dmg` (5.3MB)
3. GitHub Releases에 DMG 업로드 (v0.1.0)
   - URL: `https://github.com/robinbaebae/SPRT/releases/download/v0.1.0/SPRT_0.1.0_aarch64.dmg`

### Phase 3: 랜딩페이지
1. `landing/index.html` (영문) 제작
   - 다크 테마, Sora 폰트
   - Hero → Features → Setup Guide → Footer 구조
   - 다운로드 버튼 → GitHub Releases DMG 직접 링크
   - scroll-reveal 애니메이션 (IntersectionObserver)
2. `landing/ko.html` (한국어) 제작 — 동일 구조, 한국어 번역
3. 로고 이미지 교체 (base64 인라인)
   - nav bar: 28×28px 표시
   - hero: 120×120px 표시
   - 원본: 570×570 PNG (고해상도 Retina 대응)
4. Hero 섹션 구조 확정:
   - badge ("macOS menu bar app") → 로고 이미지 → 메탈릭 서브타이틀 → 설명 → CTA 버튼
5. Setup Guide 섹션:
   - 요구사항: macOS 13+ (Apple Silicon), Claude Code CLI
   - 설치 순서: DMG 다운로드 → 마운트 → Applications 복사
   - 보안 해제: `xattr -cr /Applications/SPRT.app` 또는 System Settings → Privacy & Security
6. GitHub URL 통일: `robinbaebae/SPRT`

---

## 6. 개발 & 빌드 명령어

```bash
# 개발 서버 (Tauri + Vite HMR)
npm run tauri dev

# 프로덕션 빌드 (DMG 생성)
npm run tauri build
# → src-tauri/target/release/bundle/dmg/SPRT_0.1.0_aarch64.dmg

# Vite만 실행 (프론트엔드 개발)
npm run dev        # localhost:1420

# 타입 체크
tsc --noEmit
```

---

## 7. macOS 설치 & 보안

서명되지 않은 앱이므로 설치 후 보안 해제 필요:

```bash
# 방법 1: 터미널
xattr -cr /Applications/SPRT.app

# 방법 2: System Settings
# System Settings → Privacy & Security → "SPRT" was blocked → Open Anyway
```

---

## 8. 디자인 시스템

### 색상 (CSS Variables)
```css
/* 라이트 */
--text: #232428;  --text2: #555555;  --text3: #999999;
--accent: #232428;  --green: #22c55e;
--glass: rgba(255,255,255,0.35);

/* 다크 */
--text: #f0f0f0;  --text2: #aaaaaa;  --text3: #666666;
--accent: #ffffff;  --green: #4ade80;
--glass: rgba(255,255,255,0.06);
```

### 배경 그라디언트
- 라이트: `#d8dce4` base + 블루-그레이 radial gradients
- 다크: `#0c0c10` base + 네이비 radial gradients

### 폰트
- Sora (400, 500, 600, 700, 800)

### 글래스모피즘
- `backdrop-filter: blur(20px)`
- 반투명 배경 + 얇은 border

---

## 9. 랜딩페이지 수정 가이드

### 로고 교체
로고는 base64로 인라인. 교체 시:
```bash
# 새 로고 base64 변환
base64 -i new-logo.png | tr -d '\n'

# Python으로 양쪽 파일 일괄 교체
python3 -c "
import base64, re
with open('new-logo.png', 'rb') as f:
    new = base64.b64encode(f.read()).decode()
for p in ['landing/index.html', 'landing/ko.html']:
    with open(p) as f: c = f.read()
    c = re.sub(r'data:image/png;base64,[A-Za-z0-9+/=]+', f'data:image/png;base64,{new}', c)
    with open(p, 'w') as f: f.write(c)
"
```

### 서브타이틀 메탈릭 효과 (CSS)
```css
.hero-sub {
  font-size: clamp(20px, 4vw, 32px);
  font-weight: 800;
  background: linear-gradient(180deg, #fff 0%, #e0e0e0 30%, #aaa 50%, #ccc 70%, #fff 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}
```

### 다운로드 링크
```
DMG 직접: https://github.com/robinbaebae/SPRT/releases/download/v0.1.0/SPRT_0.1.0_aarch64.dmg
Releases: https://github.com/robinbaebae/SPRT/releases/latest
```

---

## 10. 알려진 이슈 & TODO

- [ ] 랜딩 `landing/images/` 스크린샷 4장 있지만 미사용 (제거 검토)
- [ ] 앱 서명/공증 미적용 (xattr 필요)
- [ ] DevLog AI 호출 시 Claude API 키 필요 (사용자 환경변수)
- [ ] 버전 업데이트 시: `package.json`, `tauri.conf.json`, `Cargo.toml` 세 곳 동기화 필요
- [ ] 랜딩페이지 배포 (GitHub Pages 설정 또는 별도 호스팅)

---

## 11. GitHub Releases 관리

```bash
# DMG 빌드
npm run tauri build

# GitHub Release에 DMG 업로드 (gh CLI)
gh release upload v0.1.0 src-tauri/target/release/bundle/dmg/SPRT_0.1.0_aarch64.dmg --clobber

# 새 릴리즈 생성
gh release create v0.2.0 src-tauri/target/release/bundle/dmg/SPRT_0.2.0_aarch64.dmg --title "v0.2.0" --notes "Release notes"
```

---

## 12. 핵심 파일 빠른 참조

| 수정 대상 | 파일 |
|-----------|------|
| 대시보드 UI/로직 | `src/App.tsx` |
| 앱 스타일 | `src/App.css` |
| Claude 데이터 파싱 | `src-tauri/src/claude.rs` |
| 트레이/윈도우 설정 | `src-tauri/src/lib.rs` |
| 윈도우 크기/설정 | `src-tauri/tauri.conf.json` |
| DevLog 생성 | `src-tauri/src/devlog.rs` |
| Git 파싱 | `src-tauri/src/git.rs` |
| 랜딩 (EN) | `landing/index.html` |
| 랜딩 (KO) | `landing/ko.html` |
| 앱 아이콘 | `src-tauri/icons/` |
| 트레이 아이콘 | `src-tauri/icons/tray-icon.png` |
