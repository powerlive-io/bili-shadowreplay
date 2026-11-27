---
title: "编写测试与维护数据"
description: "单元测试、HTTP 模拟、测试数据更新和确定性测试规范。"
---

## 测试放置与基本规范

- Rust 单元测试放在对应模块的 `#[cfg(test)] mod tests` 中，集成测试放在对应包的 `tests/` 下。
- 前端逻辑和开发脚本测试放在 `scripts/tests/*.test.mjs`，由 `yarn test` 自动发现。
- 普通回归不访问生产平台 API；使用本地测试数据和模拟 HTTP 服务。真实网络测试单独标记为 `#[ignore]`，注明执行条件。
- 除成功路径外，还应覆盖无效输入、失败、取消和清理。涉及文件输出时，验证失败不会破坏已有结果。
- 使用独立临时目录并可靠清理；异步测试使用正确的运行时，网络与任务等待应有超时。
- 避免依赖真实时间、直播间状态或不稳定的调度顺序。耗时、硬件和模型测试应与快速回归分离。

## 单元测试

以下是模块内测试的结构示例，`my_function` 代表被测函数：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        assert_eq!(my_function(5), 10);
    }
}
```

集成测试放在例如 `src-tauri/crates/recorder/tests/my_test.rs` 中，通过包的公开接口验证行为。异步场景使用 `#[tokio::test]`。

## 模拟 HTTP 服务

平台 API 测试使用 [wiremock](https://docs.rs/wiremock/) 模拟 HTTP 响应：

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn test_with_mock_server() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/endpoint"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&mock_server)
        .await;

    // 将 mock_server.uri() 传给可配置基础地址的适配器方法，
    // 例如 get_room_info_with_base(...)，再断言解析结果和错误行为。
}
```

示例只展示测试结构。编写用例时，要调用实际被测接口并添加断言。模拟服务除了返回响应，还应校验收到的请求方法、路径和参数。

## 维护平台测试数据

平台响应位于 `src-tauri/crates/recorder/tests/fixtures/`，媒体样本位于 `src-tauri/tests/video/`。

平台数据来自与 `recorder::platforms::*::api` 相同请求形式的真实 API／页面响应，采集脚本会脱敏部分短期查询令牌。现有数据包括：

| 文件 | 内容 |
| --- | --- |
| `bilibili_room_info.json`、`bilibili_play_url.json`、`bilibili_user_info.json` | 哔哩哔哩房间、播放地址与用户信息 |
| `douyin_room_info.json`、`douyin_room_offline.json` | 抖音 H5 reflow API 的在线与离线响应 |
| `huya_room_page.html` | `m.huya.com` 页面及 `HNF_GLOBAL_INIT` |
| `kuaishou_initial_state.json`、`kuaishou_room_*.json` | 快手页面初始状态与房间响应 |
| `test_playlist.m3u8` | 真实哔哩哔哩 fMP4 HLS 播放列表片段 |

### 更新数据

这是需要联网的维护操作，不属于普通测试运行。先在本地环境中安全提供 `TEST_BILIBILI_COOKIE` 和 `TEST_DOUYIN_COOKIE`，再从仓库根目录运行：

```bash
python3 scripts/fetch_test_fixtures.py
```

不要把真实 Cookie、令牌或其他凭据写入仓库文件、命令示例或日志。采集后先人工检查差异和脱敏结果，确认没有敏感字段、短期签名地址或无关个人信息，再运行使用这些数据的测试。未经检查的数据不要提交。

### 读取数据

通过 `CARGO_MANIFEST_DIR` 定位，避免依赖测试启动目录：

```rust
fn load_fixture(name: &str) -> String {
    let fixture_path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read_to_string(fixture_path).unwrap()
}
```

## 参考资料

- [Rust 测试章节](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [wiremock 文档](https://docs.rs/wiremock/)
- [Tokio 测试指南](https://tokio.rs/tokio/topics/testing)

测试基础设施问题可通过仓库 Issue 反馈；请附执行命令、系统环境和已脱敏的错误输出。
