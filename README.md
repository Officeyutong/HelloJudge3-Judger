# HelloJudge3-Judger

HJ2/HJ3评测器，由Rust强力驱动，兼容HJ2的通信协议。

## 前置需求

使用docker目录下的Dockerfile构建一个用以评测的docker镜像

- 评测镜像也通过`officeyutong/hj3-judger-container`提供，但不一定及时更新

## 系统要求

理论上安装有Docker的x86_64 Linux都支持。

仅在Ubuntu 20.04下测试过。

## 已实现功能
- 传统题评测（包括SPJ题目）
- 提交答案题评测
- 在线IDE运行

## 部署方式

- 从actions或release中下载二进制文件
- 运行下载的可执行文件，并让他生成默认配置文件后退出
- 修改配置文件
- 再次运行，即可正常使用

## 配置文件

```yaml
# celery消息中间人URL
broker_url: "redis://127.0.0.1/4"
# 测试数据存放文件夹
data_dir: testdata
# HJ2/HJ3 API服务器地址，必须以斜线结尾
web_api_url: "http://192.168.56.1:8095/"
# HJ2评测机Token
judger_uuid: 14ece11c-c98e-11e9-9133-9cda3efd56be
# 用以评测的Docker镜像
docker_image: "aae4f7819e09"
# 日志等级
logging_level: debug
# 预创建的工作线程数量，由于rusty-celery的限制，至少需要为2
prefetch_count: 2
# 同时允许的最大评测任务数
max_tasks_sametime: 1
```