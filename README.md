# fileserver

[English](#english) | [한국어](#한국어)

---

## 한국어

웹 브라우저로 접속하는 간단한 파일 서버입니다. 드라이브나 폴더를 지정하면 같은 네트워크의 다른 기기에서 브라우저로 파일을 탐색하고 내려받을 수 있습니다. Rust(axum) 로 작성된 단일 실행 파일입니다.

### 주요 기능

- 여러 폴더/드라이브를 동시에 공유 (`이름=경로` 형식으로 공유 이름 지정)
- 브라우저 기반 탐색 UI
- Range 요청 지원 (이어받기, 동영상 탐색)
- 선택적 Basic 인증 (`--password`)
- 선택적 업로드 (`--upload`, 기본은 읽기 전용)
- 경로 이탈(`..`) 방지

### 빌드

[Rust](https://www.rust-lang.org/tools/install) (edition 2024 지원 버전) 가 필요합니다.

```powershell
git clone https://github.com/civilian7/fileserver.git
cd fileserver
cargo build --release
```

실행 파일은 `target/release/fileserver.exe` 에 생성됩니다.

### 기본 사용 방법

```powershell
# 1. 폴더 하나를 읽기 전용으로 공유 (인증 없음)
fileserver D:\Share

# 2. 드라이브 전체 공유
fileserver D:\

# 3. 여러 폴더를 이름을 붙여 공유
fileserver 문서=C:\Users\me\Documents 사진=D:\Photos

# 4. 암호를 걸고 업로드 허용, 포트 변경
fileserver D:\Share --user admin --password 1234 --upload --port 9000

# 5. 이 PC 에서만 접속 허용
fileserver D:\Share --bind 127.0.0.1
```

실행하면 접속 주소가 출력됩니다.

```text
공유 목록:
  Share -> D:\Share
모드: 읽기 전용, 인증: 없음 (누구나 접속 가능)
접속 주소:
  http://localhost:8080
  http://192.168.0.10:8080
종료: Ctrl+C
```

브라우저에서 해당 주소를 열면 됩니다. 개발 중에는 `cargo run -- D:\Share` 로도 실행할 수 있습니다.

### 옵션

| 옵션 | 기본값 | 설명 |
|---|---|---|
| `<경로>...` | (필수) | 공유할 드라이브/폴더. `경로` 또는 `이름=경로` |
| `-b, --bind <IP>` | `0.0.0.0` | 수신 주소. 이 PC 에서만 쓰려면 `127.0.0.1` |
| `-p, --port <PORT>` | `8080` | 포트 |
| `-u, --user <USER>` | `admin` | 접속 사용자 이름 (`--password` 와 함께 사용) |
| `-P, --password <PW>` | 없음 | 접속 암호. 지정하지 않으면 인증 없이 공유 |
| `--upload` | 꺼짐 | 브라우저에서 업로드 허용 |

### API

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/` | 웹 UI |
| GET | `/api/info` | 서버 정보 |
| GET | `/api/list/{share}/{path}` | 폴더 목록 (JSON) |
| GET | `/files/{share}/{path}` | 파일 다운로드 |
| POST | `/api/upload/{share}/{path}` | 업로드 (`--upload` 필요, multipart) |

```powershell
curl -u admin:1234 http://localhost:8080/api/list/Share
curl -u admin:1234 -O http://localhost:8080/files/Share/readme.txt
curl -u admin:1234 -F "file=@local.txt" http://localhost:8080/api/upload/Share
```

### 보안 주의

- 암호 없이 `0.0.0.0` 으로 열면 같은 네트워크의 누구나 접근할 수 있습니다.
- Basic 인증은 암호화되지 않습니다. 신뢰할 수 있는 네트워크에서만 쓰거나 HTTPS 리버스 프록시 뒤에 두세요.
- 인터넷에 직접 노출하지 마세요.

---

## English

A simple file server you use from a web browser. Point it at drives or folders and browse or download files from any device on the same network. It ships as a single executable written in Rust (axum).

### Features

- Share multiple folders/drives at once (name them with `name=path`)
- Browser-based file browsing UI
- Range request support (resumable downloads, video seeking)
- Optional Basic authentication (`--password`)
- Optional uploads (`--upload`; read-only by default)
- Path traversal (`..`) protection

### Build

Requires [Rust](https://www.rust-lang.org/tools/install) with edition 2024 support.

```powershell
git clone https://github.com/civilian7/fileserver.git
cd fileserver
cargo build --release
```

The binary is produced at `target/release/fileserver.exe`.

### Basic usage

```powershell
# 1. Share one folder, read-only, no authentication
fileserver D:\Share

# 2. Share an entire drive
fileserver D:\

# 3. Share several folders with custom names
fileserver docs=C:\Users\me\Documents photos=D:\Photos

# 4. Require a password, allow uploads, change the port
fileserver D:\Share --user admin --password 1234 --upload --port 9000

# 5. Accept connections from this PC only
fileserver D:\Share --bind 127.0.0.1
```

On startup the server prints the addresses to open:

```text
공유 목록:
  Share -> D:\Share
모드: 읽기 전용, 인증: 없음 (누구나 접속 가능)
접속 주소:
  http://localhost:8080
  http://192.168.0.10:8080
종료: Ctrl+C
```

(The console messages are in Korean: share list, mode (read-only), authentication (none), addresses, quit with Ctrl+C.)

Open the address in a browser. During development you can also run `cargo run -- D:\Share`.

### Options

| Option | Default | Description |
|---|---|---|
| `<PATH>...` | (required) | Drive/folder to share. `path` or `name=path` |
| `-b, --bind <IP>` | `0.0.0.0` | Listen address. Use `127.0.0.1` for local-only |
| `-p, --port <PORT>` | `8080` | Port |
| `-u, --user <USER>` | `admin` | Username (used together with `--password`) |
| `-P, --password <PW>` | none | Password. Without it, no authentication is required |
| `--upload` | off | Allow uploads from the browser |

### API

| Method | Path | Description |
|---|---|---|
| GET | `/` | Web UI |
| GET | `/api/info` | Server info |
| GET | `/api/list/{share}/{path}` | Directory listing (JSON) |
| GET | `/files/{share}/{path}` | Download a file |
| POST | `/api/upload/{share}/{path}` | Upload (requires `--upload`, multipart) |

```powershell
curl -u admin:1234 http://localhost:8080/api/list/Share
curl -u admin:1234 -O http://localhost:8080/files/Share/readme.txt
curl -u admin:1234 -F "file=@local.txt" http://localhost:8080/api/upload/Share
```

### Security notes

- Without a password on `0.0.0.0`, anyone on your network can access the shared files.
- Basic authentication is not encrypted. Use it only on trusted networks or behind an HTTPS reverse proxy.
- Do not expose it directly to the internet.
