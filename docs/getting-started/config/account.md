---
title: "账号配置"
---


添加 B 站或抖音直播间前，请先到账号页面点击「添加账号」，配置对应平台的账号。斗鱼和 Twitch 不需要账号；虎牙、快手、TikTok 和 YouTube 的账号 Cookie 可按需填写。

- B 站账号：目前支持扫码登录和 Cookie 手动配置两种方式，推荐使用扫码登录
- 抖音账号：目前仅支持手动配置 Cookie 登录

## 账号凭据存储

桌面版默认启用 `credential-encryption` 编译特性，Cookie 和 CSRF Token 使用 AES-256-GCM 加密后写入 SQLite，已有明文账号会在启动时自动迁移。主密钥保存在系统凭据存储中：macOS Keychain、Windows Credential Manager 或 Linux Secret Service。Linux 需要安装并解锁 Secret Service；系统凭据存储不可用时，启用加密的程序会启动失败。

Docker、Termux 和默认无界面构建使用 `--no-default-features --features headless`，关闭加密并以明文保存账号凭据，无需系统密钥库。系统支持密钥库时，可以使用 `--no-default-features --features headless,credential-encryption` 构建启用加密的无界面版本。桌面版也可使用 `--no-default-features --features gui` 关闭加密。

关闭加密的版本不能打开包含已加密账号的数据库，需要改用启用 `credential-encryption` 的版本。恢复加密数据库时仍需访问系统密钥库中的原主密钥；仅复制数据库到其他设备无法恢复账号。升级前的备份仍可能含有明文凭据。

桌面版遇到密钥缺失、钥匙串访问失败或已有密文无法解密时，会显示具体原因。**要保留数据，选“退出并配置钥匙串”**，恢复对原钥匙串的访问后再启动。“清空所有登录信息并重启”会删除已保存的账号，不会删除或修复已加密的存储凭据，也不会解决钥匙串访问问题。

### 密钥生命周期

启用加密时，账号和远端录制存储凭据共用系统钥匙串中的主密钥。正常退出不会删除或轮换密钥，下次启动会重新读取它。

[密钥决策图：哪些情况可以创建密钥，哪些情况必须停止](https://bsr.powerlive.io/diagrams/account-key.html)

## 抖音账号配置

登录抖音并打开[个人主页](https://www.douyin.com/user/self)。右键单击网页，选择 `检查（Inspect）` 打开开发者工具。切换到 `网络（Network）` 选项卡后刷新网页，在列表中选择 `self` 请求（通常是第一项）。展开 `请求标头`，从 `请求标头` 中复制 `Cookie` 字段的完整值，粘贴到配置页的 `Cookie` 输入框。

![DouyinCookie](/images/douyin_cookie.png)
