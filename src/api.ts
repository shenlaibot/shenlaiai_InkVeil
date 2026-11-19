// Tauri 命令的 thin 封装（invoke 透传）。
import { invoke } from "@tauri-apps/api/core";
import type {
  AuditEntry,
  AuditVerify,
  CollectResult,
  DownloadStatus,
  ModelEntry,
  RedactInput,
  RedactOptions,
  RedactResult,
  RestoreResult,
  FuzzyRestoreResult,
  DocRestoreResult,
  RuleDef,
  VaultInfo,
  ScanOptions,
  ScanResult,
} from "./types";

// ---- 文件导入 ----
export const pickFiles = () => invoke<string[]>("pick_files");
export const pickFolder = () => invoke<string | null>("pick_folder");
export const pickMappingFile = () => invoke<string | null>("pick_mapping_file");
export const pickOutputDir = () => invoke<string | null>("pick_output_dir");
/** 展开文件夹 + 统计大小/修改时间；exclude 为已记录的输出文件/目录（文件夹展开时排除导出产物） */
export const collectFiles = (paths: string[], exclude: string[] = []) =>
  invoke<CollectResult>("collect_files", { paths, exclude });
export const openInFolder = (path: string) => invoke<void>("open_in_folder", { path });
export const saveTextFile = (defaultName: string, content: string) =>
  invoke<string | null>("save_text_file", { defaultName, content });

// ---- 扫描 / 脱敏 / 还原 ----
export const getBuiltinRules = () => invoke<RuleDef[]>("get_builtin_rules");
export const nerModelPath = () => invoke<string | null>("ner_model_path");
export const scanFiles = (paths: string[], options: ScanOptions) =>
  invoke<ScanResult>("scan_files", { paths, options });
/** 取消正在进行的扫描：后端每次 scan_files 调用都会让旧扫描在下一个文件边界停止，空列表即“只取消” */
export const cancelScan = (options: ScanOptions) =>
  invoke<ScanResult>("scan_files", { paths: [], options });
/** 后端取消扫描时返回的错误文本（与 commands.rs SCAN_CANCELLED 一致） */
export const SCAN_CANCELLED = "扫描已取消";
export const confirmRedact = (
  inputs: RedactInput[],
  outputDir: string,
  options: RedactOptions,
) => invoke<RedactResult>("confirm_redact", { inputs, outputDir, options });
export const restoreText = (text: string, mappingPath: string, passphrase: string | null) =>
  invoke<RestoreResult>("restore_text", { text, mappingPath, passphrase });
export const restoreTextFuzzy = (text: string, mappingPath: string, passphrase: string | null) =>
  invoke<FuzzyRestoreResult>("restore_text_fuzzy", { text, mappingPath, passphrase });
export const pickRestoreDoc = () => invoke<string | null>("pick_restore_doc");
export const restoreDocument = (
  inputPath: string,
  outputDir: string,
  mappingPath: string,
  passphrase: string | null,
) => invoke<DocRestoreResult>("restore_document", { inputPath, outputDir, mappingPath, passphrase });

// ---- 项目保险库 ----
export const pickVaultFile = () => invoke<string | null>("pick_vault_file");
export const vaultCreate = (dir: string, name: string, passphrase: string) =>
  invoke<VaultInfo>("vault_create", { dir, name, passphrase });
export const vaultOpen = (path: string, passphrase: string) =>
  invoke<VaultInfo>("vault_open", { path, passphrase });
export const vaultInfo = () => invoke<VaultInfo | null>("vault_info");
export const vaultClose = () => invoke<void>("vault_close");
export const vaultSetDict = (terms: string[]) => invoke<VaultInfo>("vault_set_dict", { terms });

// ---- 模型市场 ----
export const listModels = () => invoke<ModelEntry[]>("list_models");
export const modelStatus = (modelId: string) =>
  invoke<DownloadStatus>("model_status", { modelId });
export const startModelDownload = (modelId: string, consent?: boolean) =>
  invoke<void>("start_model_download", { modelId, consent });
export const pauseModelDownload = (modelId: string) =>
  invoke<void>("pause_model_download", { modelId });
export const deleteModel = (modelId: string) => invoke<void>("delete_model", { modelId });
export const modelInstallDir = (modelId: string) => invoke<string>("model_install_dir", { modelId });

// ---- 审计日志与应用配置（SQLite） ----
export const auditList = (limit?: number) => invoke<AuditEntry[]>("audit_list", { limit });
export const auditAdd = (entry: AuditEntry) => invoke<void>("audit_add", { entry });
export const auditVerify = () => invoke<AuditVerify>("audit_verify");
export const auditPrune = (retentionDays: number) => invoke<number>("audit_prune", { retentionDays });
export const auditRetentionGet = () => invoke<number>("audit_retention_get");
export const auditRetentionSet = (days: number) => invoke<void>("audit_retention_set", { days });
export const configGet = () => invoke<string | null>("config_get");
export const configSet = (json: string) => invoke<void>("config_set", { json });
