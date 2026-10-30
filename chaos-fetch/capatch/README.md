# capatch 模块

往 **libcurl 真正使用的全局 CA 链**里追加根证书，修好 HTTPS 的 `code 60`（证书校验失败）。

## 它改了什么

| 位置 | 动作 | 生命周期 |
|---|---|---|
| `*(0x20103594)` | 全局 CA 链指针；为空时 `malloc(0x154)` + `mbedtls_x509_crt_init` 建一条 | RAM |
| `/data/ssl/ca.pem` | 读入并 `parse_file` 到上面那条链 | 文件 |
| `0x200c9bb8/0x200c9bbc` | libcurl 的 CAfile/CApath 字符串指针（诊断用）| RAM |

**不是 `-k`**：校验照常进行，只是把被裁掉的根证书补回去。

## 构建

```bash
python build_capatch.py     # 需要 NDK clang（同 tun 模块）
# 产物 capatch.elf
```

## 节点

`/dev/capatch`（只读诊断）：`cat` 出链状态、`pret`（parse_file 返回值）、
`chain/t/l/p` 三个链字段。`pret=0` 表示证书已进链。

## 注意

加载时会读 `/data/ssl/ca.pem`——**文件不在就什么都不做**（`pret=-777`），不会报错也不会崩。
