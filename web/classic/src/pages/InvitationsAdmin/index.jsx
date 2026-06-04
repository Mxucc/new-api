/*
Copyright (C) 2025 QuantumNous

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as
published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.

For commercial licensing, please contact support@quantumnous.com
*/

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Card,
  Checkbox,
  Col,
  Empty,
  Input,
  InputNumber,
  Modal,
  Pagination,
  Row,
  Select,
  Space,
  Spin,
  Switch,
  Table,
  TabPane,
  Tabs,
  Tag,
  TextArea,
  Typography,
} from '@douyinfe/semi-ui';
import {
  Ban,
  BarChart3,
  Check,
  CheckCircle,
  Edit,
  Gift,
  LockOpen,
  Plus,
  ReceiptText,
  RotateCcw,
  Save,
  Settings,
  TimerOff,
  TimerReset,
  Trash2,
  UserRoundPlus,
  XCircle,
} from 'lucide-react';
import { showError, showSuccess } from '../../helpers';
import {
  INVITATION_FEATURE_STATUS_REFRESH_EVENT,
  invitationApi,
  invitationAdminApi,
} from '../../components/invitations/api';
import {
  ALL_USER_GROUP,
  DEFAULT_PAGE_SIZE,
  addDaysToDatetimeLocal,
  centsToDisplayAmount,
  collectRecordIds,
  displayAmountToCents,
  extractData,
  formatDateTime,
  formatPercent,
  formatRebateAmount,
  formatUser,
  getRecordId,
  groupNameLabel,
  invitationErrorMessage,
  orderTypeLabel,
  requestStatusLabel,
  timestampToDateTime,
  toDatetimeLocalValue,
  adminOrderStatusLabel,
  userRebateStatusLabel,
  isInvitationSignupReward,
} from '../../components/invitations/utils';

const { Text, Title } = Typography;

const DEFAULT_TAB = 'rules';

function TabLabel({ icon: Icon, children }) {
  return (
    <span className='inline-flex items-center gap-1'>
      <Icon size={15} />
      {children}
    </span>
  );
}

function formatPageTotal(t, page, pageSize, total) {
  if (!total) return '';
  const start = (page - 1) * pageSize + 1;
  const end = Math.min(page * pageSize, total);
  return `${t('显示第')} ${start} ${t('条 - 第')} ${end} ${t('条，共')} ${total} ${t('条')}`;
}

function useInvitationBackendAvailable() {
  const [available, setAvailable] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;

    async function loadStatus() {
      try {
        const response = await invitationApi.getStatus();
        if (active)
          setAvailable(Boolean(response?.success && response?.data?.available));
      } catch (error) {
        if (active) setAvailable(false);
      } finally {
        if (active) setLoading(false);
      }
    }

    loadStatus();

    return () => {
      active = false;
    };
  }, []);

  return { available, loading };
}

function buildDefaultConfig() {
  return {
    minRebateRequestAmount: 0,
    rebateRequestFrequencyDays: 1,
    userInvitationRebateEnabled: true,
    orderRebateEnabled: false,
    invitationSignupRewardEnabled: false,
    invitationSignupRewardAmount: 0,
    invitationSignupInviterRewardAmount: 0,
    invitationSignupInviteeRewardAmount: 0,
    invitationSignupRewardReviewRequired: true,
    invitationSignupInviterRewardRequiresPaidOrder: false,
    invitationSignupInviteeRewardRequiresPaidOrder: false,
    rebateToBalanceEnabled: false,
  };
}

function configToForm(config) {
  const next = { ...buildDefaultConfig(), ...(config || {}) };
  return {
    ...next,
    minRebateRequestAmountDisplay: centsToDisplayAmount(
      next.minRebateRequestAmount,
    ),
    invitationSignupRewardAmountDisplay: centsToDisplayAmount(
      next.invitationSignupRewardAmount,
    ),
    invitationSignupInviterRewardAmountDisplay: centsToDisplayAmount(
      next.invitationSignupInviterRewardAmount ??
        next.invitationSignupRewardAmount,
    ),
    invitationSignupInviteeRewardAmountDisplay: centsToDisplayAmount(
      next.invitationSignupInviteeRewardAmount ??
        next.invitationSignupRewardAmount,
    ),
  };
}

function formToConfig(form) {
  const inviterAmount = displayAmountToCents(
    form.invitationSignupInviterRewardAmountDisplay,
  );
  const inviteeAmount = displayAmountToCents(
    form.invitationSignupInviteeRewardAmountDisplay,
  );

  return {
    minRebateRequestAmount: displayAmountToCents(
      form.minRebateRequestAmountDisplay,
    ),
    rebateRequestFrequencyDays: Number(form.rebateRequestFrequencyDays ?? 0),
    userInvitationRebateEnabled: Boolean(form.userInvitationRebateEnabled),
    orderRebateEnabled: Boolean(form.orderRebateEnabled),
    invitationSignupRewardEnabled: Boolean(form.invitationSignupRewardEnabled),
    invitationSignupRewardAmount:
      inviterAmount === 0 && inviteeAmount === 0
        ? 0
        : Math.max(inviterAmount, inviteeAmount),
    invitationSignupInviterRewardAmount: inviterAmount,
    invitationSignupInviteeRewardAmount: inviteeAmount,
    invitationSignupRewardReviewRequired: Boolean(
      form.invitationSignupRewardReviewRequired,
    ),
    invitationSignupInviterRewardRequiresPaidOrder: Boolean(
      form.invitationSignupInviterRewardRequiresPaidOrder,
    ),
    invitationSignupInviteeRewardRequiresPaidOrder: Boolean(
      form.invitationSignupInviteeRewardRequiresPaidOrder,
    ),
    rebateToBalanceEnabled: Boolean(form.rebateToBalanceEnabled),
  };
}

function SettingSwitch({ title, description, checked, onChange }) {
  return (
    <div
      className='flex min-h-[88px] items-center justify-between gap-4 rounded-xl p-4'
      style={{
        border: '1px solid var(--semi-color-border)',
        background: 'var(--semi-color-fill-0)',
      }}
    >
      <div>
        <div className='font-medium'>{title}</div>
        <Text type='secondary' size='small'>
          {description}
        </Text>
      </div>
      <Switch checked={checked} onChange={onChange} />
    </div>
  );
}

function SystemConfigCard() {
  const { t } = useTranslation();
  const [config, setConfig] = useState(null);
  const [form, setForm] = useState(configToForm(null));
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  const loadConfig = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationAdminApi.getSystemConfig();
      const data = extractData(response, buildDefaultConfig());
      setConfig(data);
      setForm(configToForm(data));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载系统配置失败')));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const updateForm = (key, value) => {
    setForm((current) => ({ ...current, [key]: value }));
  };

  const resetForm = () => {
    setForm(configToForm(config));
  };

  const saveConfig = async () => {
    const payload = formToConfig(form);
    if (payload.rebateRequestFrequencyDays < 0) {
      showError(t('返利申请频率不能为负数'));
      return;
    }

    setSaving(true);
    try {
      const response = await invitationAdminApi.updateSystemConfig(payload);
      const data = extractData(response, payload);
      setConfig(data);
      setForm(configToForm(data));
      window.dispatchEvent(new Event(INVITATION_FEATURE_STATUS_REFRESH_EVENT));
      showSuccess(t('系统配置已保存'));
    } catch (error) {
      showError(invitationErrorMessage(error, t('保存系统配置失败')));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card
      className='w-full min-w-0 !rounded-2xl'
      title={t('系统配置')}
      bordered
      style={{ width: '100%' }}
      headerExtraContent={
        <Space>
          <Button
            icon={<RotateCcw size={15} />}
            onClick={resetForm}
            disabled={saving}
          >
            {t('重置')}
          </Button>
          <Button
            type='primary'
            icon={<Save size={15} />}
            loading={saving}
            onClick={saveConfig}
          >
            {t('保存')}
          </Button>
        </Space>
      }
    >
      <Spin spinning={loading}>
        <Row gutter={[16, 16]}>
          <Col xs={24} md={12}>
            <Text strong>{t('最低返利到余额金额')}</Text>
            <InputNumber
              className='mt-2 w-full'
              min={0}
              step={0.01}
              precision={2}
              value={form.minRebateRequestAmountDisplay}
              onChange={(value) =>
                updateForm('minRebateRequestAmountDisplay', value)
              }
            />
            <Text type='secondary' size='small'>
              {t('当前示例：')}100 {t('分')} = {formatRebateAmount(100)}
            </Text>
          </Col>
          <Col xs={24} md={12}>
            <Text strong>{t('返利申请间隔（天）')}</Text>
            <InputNumber
              className='mt-2 w-full'
              min={0}
              step={1}
              value={form.rebateRequestFrequencyDays}
              onChange={(value) =>
                updateForm('rebateRequestFrequencyDays', value)
              }
            />
            <Text type='secondary' size='small'>
              {t('两次返利到余额申请之间的最小天数，0 表示不限制')}
            </Text>
          </Col>
          <Col xs={24}>
            <SettingSwitch
              title={t('用户邀请返利')}
              description={t('关闭后用户侧邀请返利界面不显示')}
              checked={form.userInvitationRebateEnabled}
              onChange={(value) =>
                updateForm('userInvitationRebateEnabled', value)
              }
            />
          </Col>
          <Col xs={24} md={12}>
            <SettingSwitch
              title={t('订单返利')}
              description={t('根据充值和订阅订单生成返利记录')}
              checked={form.orderRebateEnabled}
              onChange={(value) => updateForm('orderRebateEnabled', value)}
            />
          </Col>
          <Col xs={24} md={12}>
            <SettingSwitch
              title={t('返利到余额')}
              description={t('允许用户把可返利金额申请到余额')}
              checked={form.rebateToBalanceEnabled}
              onChange={(value) => updateForm('rebateToBalanceEnabled', value)}
            />
          </Col>
          <Col xs={24} md={12}>
            <Text strong>{t('邀请者注册奖励金额')}</Text>
            <InputNumber
              className='mt-2 w-full'
              min={0}
              step={0.01}
              precision={2}
              value={form.invitationSignupInviterRewardAmountDisplay}
              onChange={(value) =>
                updateForm('invitationSignupInviterRewardAmountDisplay', value)
              }
            />
            <Text type='secondary' size='small'>
              {t('当前示例：')}200 {t('分')} = {formatRebateAmount(200)}
            </Text>
          </Col>
          <Col xs={24} md={12}>
            <Text strong>{t('被邀请者注册奖励金额')}</Text>
            <InputNumber
              className='mt-2 w-full'
              min={0}
              step={0.01}
              precision={2}
              value={form.invitationSignupInviteeRewardAmountDisplay}
              onChange={(value) =>
                updateForm('invitationSignupInviteeRewardAmountDisplay', value)
              }
            />
            <Text type='secondary' size='small'>
              {t('当前示例：')}200 {t('分')} = {formatRebateAmount(200)}
            </Text>
          </Col>
          <Col xs={24} md={12}>
            <SettingSwitch
              title={t('邀请注册奖励')}
              description={t('邀请人和被邀请的新用户生成待领取奖励')}
              checked={form.invitationSignupRewardEnabled}
              onChange={(value) =>
                updateForm('invitationSignupRewardEnabled', value)
              }
            />
          </Col>
          <Col xs={24}>
            <SettingSwitch
              title={t('邀请注册奖励审核')}
              description={t('关闭后用户领取后跳过审核，只等待管理员手动打款')}
              checked={form.invitationSignupRewardReviewRequired}
              onChange={(value) =>
                updateForm('invitationSignupRewardReviewRequired', value)
              }
            />
          </Col>
          <Col xs={24} md={12}>
            <SettingSwitch
              title={t('邀请人奖励解锁')}
              description={t('开启后需被邀请人首次充值或订阅后才可领取')}
              checked={form.invitationSignupInviterRewardRequiresPaidOrder}
              onChange={(value) =>
                updateForm(
                  'invitationSignupInviterRewardRequiresPaidOrder',
                  value,
                )
              }
            />
          </Col>
          <Col xs={24} md={12}>
            <SettingSwitch
              title={t('被邀请人奖励解锁')}
              description={t('开启后需新用户首次充值或订阅后才可领取')}
              checked={form.invitationSignupInviteeRewardRequiresPaidOrder}
              onChange={(value) =>
                updateForm(
                  'invitationSignupInviteeRewardRequiresPaidOrder',
                  value,
                )
              }
            />
          </Col>
        </Row>
      </Spin>
    </Card>
  );
}

function RuleDialog({ visible, editingRule, userGroups, onCancel, onSaved }) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const [form, setForm] = useState({
    user_group: ALL_USER_GROUP,
    rule_type: 'subscription',
    rebate_rate: '',
  });

  useEffect(() => {
    if (!visible) return;
    if (editingRule) {
      setForm({
        user_group: editingRule.user_group,
        rule_type: editingRule.rule_type,
        rebate_rate: String(Number(editingRule.rebate_rate || 0) * 100),
      });
      return;
    }
    setForm({
      user_group: ALL_USER_GROUP,
      rule_type: 'subscription',
      rebate_rate: '',
    });
  }, [editingRule, visible]);

  const submit = async () => {
    const rate = Number(form.rebate_rate);
    if (!form.user_group) {
      showError(t('请选择用户组'));
      return;
    }
    if (!Number.isFinite(rate) || rate < 0 || rate > 100) {
      showError(t('返利比例必须在 0 到 100 之间'));
      return;
    }

    const payload = {
      user_group: form.user_group,
      rule_type: form.rule_type,
      rebate_rate: String(rate / 100),
    };

    setSaving(true);
    try {
      const response = editingRule
        ? await invitationAdminApi.updateRebateRule(editingRule.id, payload)
        : await invitationAdminApi.createRebateRule(payload);
      extractData(response, null);
      showSuccess(editingRule ? t('规则已更新') : t('规则已创建'));
      onSaved();
    } catch (error) {
      showError(invitationErrorMessage(error, t('保存规则失败')));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={editingRule ? t('编辑返利规则') : t('创建返利规则')}
      visible={visible}
      onOk={submit}
      onCancel={onCancel}
      okText={t('保存')}
      cancelText={t('取消')}
      confirmLoading={saving}
      width={520}
    >
      <Space vertical spacing='medium' className='w-full'>
        <div>
          <Text strong>{t('用户组')}</Text>
          <Select
            className='mt-2 w-full'
            value={form.user_group}
            onChange={(value) =>
              setForm((current) => ({ ...current, user_group: value }))
            }
          >
            <Select.Option value={ALL_USER_GROUP}>
              {t('全部用户组')}
            </Select.Option>
            {userGroups.map((group) => (
              <Select.Option key={group.name} value={group.name}>
                {group.name} ({group.user_count} {t('人')})
              </Select.Option>
            ))}
          </Select>
        </div>
        <div>
          <Text strong>{t('规则类型')}</Text>
          <Select
            className='mt-2 w-full'
            value={form.rule_type}
            onChange={(value) =>
              setForm((current) => ({ ...current, rule_type: value }))
            }
          >
            <Select.Option value='subscription'>{t('订阅')}</Select.Option>
            <Select.Option value='topup'>{t('充值')}</Select.Option>
          </Select>
        </div>
        <div>
          <Text strong>{t('返利比例')} (%)</Text>
          <InputNumber
            className='mt-2 w-full'
            min={0}
            max={100}
            step={0.01}
            precision={2}
            value={form.rebate_rate}
            onChange={(value) =>
              setForm((current) => ({ ...current, rebate_rate: value }))
            }
            placeholder='5.00'
          />
          <Text type='secondary' size='small'>
            {t('填写百分比数值，例如 5 表示 5%')}
          </Text>
        </div>
      </Space>
    </Modal>
  );
}

function RebateRulesPanel() {
  const { t } = useTranslation();
  const [rules, setRules] = useState([]);
  const [userGroups, setUserGroups] = useState([]);
  const [loading, setLoading] = useState(false);
  const [dialogVisible, setDialogVisible] = useState(false);
  const [editingRule, setEditingRule] = useState(null);

  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const [rulesResponse, groupsResponse] = await Promise.all([
        invitationAdminApi.getRebateRules(),
        invitationAdminApi.getUserGroups(),
      ]);
      setRules(extractData(rulesResponse, []));
      setUserGroups(extractData(groupsResponse, []));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载返利规则失败')));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const deleteRule = (rule) => {
    Modal.confirm({
      title: t('删除返利规则'),
      content: t('确定要删除该返利规则吗？'),
      okText: t('删除'),
      cancelText: t('取消'),
      okType: 'danger',
      onOk: async () => {
        try {
          const response = await invitationAdminApi.deleteRebateRule(rule.id);
          extractData(response, null);
          showSuccess(t('规则已删除'));
          await loadData();
        } catch (error) {
          showError(invitationErrorMessage(error, t('删除规则失败')));
        }
      },
    });
  };

  const columns = useMemo(
    () => [
      {
        title: t('用户组'),
        dataIndex: 'user_group',
        width: 180,
        render: (value) => <Tag color='blue'>{groupNameLabel(t, value)}</Tag>,
      },
      {
        title: t('规则类型'),
        dataIndex: 'rule_type',
        width: 120,
        render: (value) => (value === 'subscription' ? t('订阅') : t('充值')),
      },
      {
        title: t('返利比例'),
        dataIndex: 'rebate_rate',
        width: 120,
        render: (value) => formatPercent(Number(value)),
      },
      {
        title: t('创建时间'),
        dataIndex: 'created_at',
        width: 220,
        render: (value) => timestampToDateTime(value),
      },
      {
        title: t('操作'),
        dataIndex: 'actions',
        fixed: 'right',
        width: 112,
        align: 'center',
        render: (_, record) => (
          <Space>
            <Button
              icon={<Edit size={15} />}
              theme='borderless'
              onClick={() => {
                setEditingRule(record);
                setDialogVisible(true);
              }}
            />
            <Button
              icon={<Trash2 size={15} />}
              theme='borderless'
              type='danger'
              onClick={() => deleteRule(record)}
            />
          </Space>
        ),
      },
    ],
    [t],
  );

  return (
    <div className='flex w-full min-w-0 flex-col gap-4'>
      <Card
        className='w-full min-w-0 !rounded-2xl'
        title={t('返利规则')}
        bordered
        style={{ width: '100%' }}
        headerExtraContent={
          <Button
            type='primary'
            icon={<Plus size={15} />}
            onClick={() => {
              setEditingRule(null);
              setDialogVisible(true);
            }}
          >
            {t('创建规则')}
          </Button>
        }
      >
        <div className='w-full min-w-0 overflow-x-auto'>
          <Table
            columns={columns}
            dataSource={rules}
            loading={loading}
            rowKey='id'
            pagination={false}
            empty={<Empty description={t('暂无返利规则')} />}
            scroll={{ x: 760 }}
          />
        </div>
      </Card>

      <SystemConfigCard />

      <RuleDialog
        visible={dialogVisible}
        editingRule={editingRule}
        userGroups={userGroups}
        onCancel={() => {
          setDialogVisible(false);
          setEditingRule(null);
        }}
        onSaved={async () => {
          setDialogVisible(false);
          setEditingRule(null);
          await loadData();
        }}
      />
    </div>
  );
}

function rowKey(record) {
  return String(getRecordId(record) || `${record.orderType}:${record.orderId}`);
}

function RebateOrderRecordsPanel() {
  const { t } = useTranslation();
  const [orderType, setOrderType] = useState('all');
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  const [data, setData] = useState({ items: [], total: 0 });
  const [loading, setLoading] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState(new Set());
  const [dialog, setDialog] = useState(null);
  const [editAmount, setEditAmount] = useState();
  const [editRatio, setEditRatio] = useState();
  const [extendUntil, setExtendUntil] = useState('');
  const [mutating, setMutating] = useState(false);

  const loadRecords = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationAdminApi.getRebateOrderRecords({
        page,
        pageSize,
        ...(orderType !== 'all' ? { orderType } : {}),
      });
      setData(extractData(response, { items: [], total: 0 }));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载返利记录失败')));
    } finally {
      setLoading(false);
    }
  }, [orderType, page, pageSize, t]);

  useEffect(() => {
    loadRecords();
  }, [loadRecords]);

  useEffect(() => {
    setSelectedKeys(new Set());
  }, [orderType, page, pageSize]);

  const selectableRecords = useMemo(
    () => data.items.filter((record) => getRecordId(record) > 0),
    [data.items],
  );

  const selectedRecords = useMemo(
    () => data.items.filter((record) => selectedKeys.has(rowKey(record))),
    [data.items, selectedKeys],
  );

  const allSelected =
    selectableRecords.length > 0 &&
    selectableRecords.every((record) => selectedKeys.has(rowKey(record)));
  const indeterminate =
    selectableRecords.some((record) => selectedKeys.has(rowKey(record))) &&
    !allSelected;

  const toggleSelectAll = (checked) => {
    setSelectedKeys((current) => {
      const next = new Set(current);
      selectableRecords.forEach((record) => {
        const key = rowKey(record);
        if (checked) next.add(key);
        else next.delete(key);
      });
      return next;
    });
  };

  const toggleRecord = (record, checked) => {
    const key = rowKey(record);
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  };

  const openDialog = (type, records) => {
    if (!records?.length) return;

    if (type === 'edit') {
      const first = records.length === 1 ? records[0] : null;
      setEditAmount(
        first
          ? Number(centsToDisplayAmount(first.rebateAmount).toFixed(2))
          : undefined,
      );
      setEditRatio(
        first?.rebateRatio != null
          ? Number((first.rebateRatio * 100).toFixed(2))
          : undefined,
      );
    }

    if (type === 'extend') {
      const first = records.length === 1 ? records[0] : null;
      const base = toDatetimeLocalValue(first?.initializationEndsAt);
      setExtendUntil(addDaysToDatetimeLocal(base, 1));
    }

    setDialog({ type, records });
  };

  const runBatch = async (operation, payload, successText, errorText) => {
    setMutating(true);
    try {
      const response = await operation(payload);
      extractData(response, null);
      showSuccess(successText);
      setDialog(null);
      setSelectedKeys(new Set());
      await loadRecords();
    } catch (error) {
      showError(invitationErrorMessage(error, errorText));
    } finally {
      setMutating(false);
    }
  };

  const currentDialogIds = () => {
    const ids = collectRecordIds(dialog?.records || []);
    if (!ids.length) {
      showError(t('所选记录缺少本地返利记录 ID，无法操作'));
    }
    return ids;
  };

  const submitEdit = () => {
    const ids = currentDialogIds();
    if (!ids.length) return;

    const payload = { recordIds: ids };
    if (editAmount !== undefined && editAmount !== null && editAmount !== '') {
      const amount = displayAmountToCents(editAmount);
      if (amount < 0) {
        showError(t('请输入有效返利金额'));
        return;
      }
      payload.rebateAmount = amount;
    }

    if (editRatio !== undefined && editRatio !== null && editRatio !== '') {
      const ratio = Number(editRatio);
      if (!Number.isFinite(ratio) || ratio < 0 || ratio > 100) {
        showError(t('返利比例必须在 0 到 100 之间'));
        return;
      }
      payload.rebateRatio = ratio / 100;
    }

    if (payload.rebateAmount == null && payload.rebateRatio == null) {
      showError(t('请填写返利金额或返利比例'));
      return;
    }

    runBatch(
      invitationAdminApi.updateRebateOrderRecords,
      payload,
      t('返利记录已更新'),
      t('更新返利记录失败'),
    );
  };

  const submitExtend = () => {
    const ids = currentDialogIds();
    if (!ids.length) return;
    const date = new Date(extendUntil);
    if (!extendUntil || Number.isNaN(date.getTime())) {
      showError(t('请输入有效初始化结束时间'));
      return;
    }
    runBatch(
      invitationAdminApi.extendRebateOrderInitialization,
      {
        recordIds: ids,
        initializationEndsAt: Math.floor(date.getTime() / 1000),
      },
      t('初始化时间已延长'),
      t('延长初始化失败'),
    );
  };

  const selectedCanModify =
    selectedRecords.length > 0 &&
    selectedRecords.every((record) => record.canModify);
  const selectedCanClose =
    selectedRecords.length > 0 &&
    selectedRecords.every((record) => record.canClose);
  const selectedCanReopen =
    selectedRecords.length > 0 &&
    selectedRecords.every((record) => record.canReopen);
  const selectedCanEnd =
    selectedRecords.length > 0 &&
    selectedRecords.every((record) => record.canEndInitialization);
  const selectedCanExtend =
    selectedRecords.length > 0 &&
    selectedRecords.every((record) => record.canExtendInitialization);

  const columns = useMemo(
    () => [
      {
        title: (
          <Checkbox
            checked={allSelected}
            indeterminate={indeterminate}
            disabled={!selectableRecords.length}
            onChange={(e) => toggleSelectAll(e.target.checked)}
          />
        ),
        dataIndex: 'select',
        width: 52,
        fixed: 'left',
        render: (_, record) => (
          <Checkbox
            checked={selectedKeys.has(rowKey(record))}
            disabled={!getRecordId(record)}
            onChange={(e) => toggleRecord(record, e.target.checked)}
          />
        ),
      },
      {
        title: t('邀请人'),
        dataIndex: 'inviterId',
        render: (value, record) => formatUser(value, record.inviterName),
      },
      {
        title: t('被邀请客户'),
        dataIndex: 'inviteeId',
        render: (value, record) => formatUser(value, record.inviteeName),
      },
      {
        title: t('用户组'),
        dataIndex: 'userGroup',
        render: (value) => <Tag color='blue'>{groupNameLabel(t, value)}</Tag>,
      },
      {
        title: t('订单类型'),
        dataIndex: 'orderType',
        render: (value) => orderTypeLabel(t, value),
      },
      {
        title: t('订单金额'),
        dataIndex: 'orderAmount',
        render: (value) => formatRebateAmount(value),
      },
      {
        title: t('返利金额'),
        dataIndex: 'rebateAmount',
        render: (value, record) => (
          <Space vertical spacing={2} align='start'>
            <span>
              {record.ruleMissing && value === 0
                ? t('未配置')
                : formatRebateAmount(value)}
            </span>
            {record.adminAdjusted && <Tag size='small'>{t('手动')}</Tag>}
          </Space>
        ),
      },
      {
        title: t('返利比例'),
        dataIndex: 'rebateRatio',
        render: (value, record) => (
          <Space>
            <span>{value == null ? t('未配置') : formatPercent(value)}</span>
            {record.ruleMissing && (
              <Tag size='small' color='yellow'>
                {t('规则缺失')}
              </Tag>
            )}
          </Space>
        ),
      },
      {
        title: t('状态'),
        dataIndex: 'status',
        render: (value) => {
          const colorMap = {
            initializing: 'blue',
            estimated: 'yellow',
            claimable: 'green',
            paid: 'grey',
            closed: 'red',
          };
          return (
            <Tag color={colorMap[value] || 'grey'}>
              {adminOrderStatusLabel(t, value)}
            </Tag>
          );
        },
      },
      {
        title: t('订单时间'),
        dataIndex: 'orderTime',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('生效时间'),
        dataIndex: 'effectiveAt',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('扫描时间'),
        dataIndex: 'scanStartedAt',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('初始化结束'),
        dataIndex: 'initializationEndsAt',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('操作'),
        dataIndex: 'actions',
        fixed: 'right',
        width: 210,
        render: (_, record) => (
          <Space spacing={2}>
            <Button
              icon={<Edit size={14} />}
              theme='borderless'
              disabled={!record.canModify}
              onClick={() => openDialog('edit', [record])}
            />
            <Button
              icon={<TimerOff size={14} />}
              theme='borderless'
              disabled={!record.canEndInitialization}
              onClick={() => openDialog('end', [record])}
            />
            <Button
              icon={<TimerReset size={14} />}
              theme='borderless'
              disabled={!record.canExtendInitialization}
              onClick={() => openDialog('extend', [record])}
            />
            <Button
              icon={<LockOpen size={14} />}
              theme='borderless'
              disabled={!record.canReopen}
              onClick={() => openDialog('reopen', [record])}
            />
            <Button
              icon={<Ban size={14} />}
              theme='borderless'
              type='danger'
              disabled={!record.canClose}
              onClick={() => openDialog('close', [record])}
            />
          </Space>
        ),
      },
    ],
    [allSelected, indeterminate, selectableRecords.length, selectedKeys, t],
  );

  return (
    <Card title={t('返利记录')} className='w-full min-w-0 !rounded-2xl'>
      <div
        className='mb-4 rounded-xl p-3'
        style={{
          position: 'sticky',
          top: 0,
          zIndex: 6,
          border: '1px solid var(--semi-color-border)',
          background: 'var(--semi-color-bg-2)',
          boxShadow: '0 4px 12px rgba(0, 0, 0, 0.06)',
        }}
      >
        <div className='flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between'>
          <div>
            <div className='font-medium'>{t('操作')}</div>
            <Text type='secondary'>
              {selectedRecords.length
                ? `${t('已选择')} ${selectedRecords.length} ${t('条')}`
                : t('未选择记录')}
            </Text>
          </div>
          <div className='flex flex-wrap items-center gap-2'>
            <Button
              icon={<Edit size={14} />}
              disabled={!selectedCanModify}
              onClick={() => openDialog('edit', selectedRecords)}
            >
              {t('批量修改')}
            </Button>
            <Button
              icon={<TimerOff size={14} />}
              disabled={!selectedCanEnd}
              onClick={() => openDialog('end', selectedRecords)}
            >
              {t('结束初始化')}
            </Button>
            <Button
              icon={<TimerReset size={14} />}
              disabled={!selectedCanExtend}
              onClick={() => openDialog('extend', selectedRecords)}
            >
              {t('延长初始化')}
            </Button>
            <Button
              icon={<LockOpen size={14} />}
              disabled={!selectedCanReopen}
              onClick={() => openDialog('reopen', selectedRecords)}
            >
              {t('开启返利')}
            </Button>
            <Button
              icon={<Ban size={14} />}
              type='danger'
              disabled={!selectedCanClose}
              onClick={() => openDialog('close', selectedRecords)}
            >
              {t('关闭返利')}
            </Button>
            <Select
              value={orderType}
              onChange={(value) => {
                setOrderType(value);
                setPage(1);
              }}
              style={{ width: 150 }}
            >
              <Select.Option value='all'>{t('全部订单')}</Select.Option>
              <Select.Option value='topup'>{t('充值')}</Select.Option>
              <Select.Option value='subscription'>{t('订阅')}</Select.Option>
            </Select>
          </div>
        </div>
      </div>

      <Table
        columns={columns}
        dataSource={data.items}
        loading={loading}
        rowKey={rowKey}
        pagination={false}
        empty={<Empty description={t('暂无返利记录')} />}
        scroll={{ x: 1780 }}
      />
      {data.total > 0 && (
        <div className='mt-4 flex flex-wrap items-center justify-between gap-3'>
          <Text type='secondary'>
            {formatPageTotal(t, page, pageSize, data.total)}
          </Text>
          <Pagination
            currentPage={page}
            pageSize={pageSize}
            total={data.total}
            showSizeChanger
            pageSizeOpts={[10, 20, 50, 100]}
            onPageChange={setPage}
            onPageSizeChange={(size) => {
              setPageSize(size);
              setPage(1);
            }}
          />
        </div>
      )}

      <Modal
        visible={Boolean(dialog)}
        onCancel={() => setDialog(null)}
        footer={null}
        width={540}
      >
        {dialog?.type === 'edit' && (
          <Space vertical spacing='medium' className='w-full'>
            <Title heading={5}>{t('修改返利记录')}</Title>
            <Text type='secondary'>
              {t('正在修改')} {dialog.records.length} {t('条返利记录')}
            </Text>
            <div>
              <Text strong>{t('返利金额')}</Text>
              <InputNumber
                className='mt-2 w-full'
                min={0}
                step={0.01}
                precision={2}
                value={editAmount}
                onChange={setEditAmount}
                placeholder='0.00'
              />
            </div>
            <div>
              <Text strong>{t('返利比例')} (%)</Text>
              <InputNumber
                className='mt-2 w-full'
                min={0}
                max={100}
                step={0.01}
                precision={2}
                value={editRatio}
                onChange={setEditRatio}
                placeholder='5.00'
              />
            </div>
            <Space className='justify-end'>
              <Button onClick={() => setDialog(null)}>{t('取消')}</Button>
              <Button type='primary' loading={mutating} onClick={submitEdit}>
                {t('保存')}
              </Button>
            </Space>
          </Space>
        )}

        {dialog?.type === 'close' && (
          <ConfirmBatchAction
            title={t('关闭返利')}
            description={t('关闭后邀请者无法看到这些订单返利记录。')}
            count={dialog.records.length}
            loading={mutating}
            okText={t('关闭')}
            okType='danger'
            onCancel={() => setDialog(null)}
            onOk={() => {
              const ids = currentDialogIds();
              if (!ids.length) return;
              runBatch(
                invitationAdminApi.closeRebateOrderRecords,
                { recordIds: ids },
                t('返利记录已关闭'),
                t('关闭返利记录失败'),
              );
            }}
          />
        )}

        {dialog?.type === 'reopen' && (
          <ConfirmBatchAction
            title={t('开启返利')}
            description={t('开启后邀请者可以再次看到这些订单返利记录。')}
            count={dialog.records.length}
            loading={mutating}
            okText={t('开启')}
            onCancel={() => setDialog(null)}
            onOk={() => {
              const ids = currentDialogIds();
              if (!ids.length) return;
              runBatch(
                invitationAdminApi.reopenRebateOrderRecords,
                { recordIds: ids },
                t('返利记录已开启'),
                t('开启返利记录失败'),
              );
            }}
          />
        )}

        {dialog?.type === 'end' && (
          <ConfirmBatchAction
            title={t('结束初始化')}
            description={t(
              '提前结束初始化后，订单会按当前返利设定进入展示状态。',
            )}
            count={dialog.records.length}
            loading={mutating}
            okText={t('结束初始化')}
            onCancel={() => setDialog(null)}
            onOk={() => {
              const ids = currentDialogIds();
              if (!ids.length) return;
              runBatch(
                invitationAdminApi.endRebateOrderInitialization,
                { recordIds: ids },
                t('初始化已结束'),
                t('结束初始化失败'),
              );
            }}
          />
        )}

        {dialog?.type === 'extend' && (
          <Space vertical spacing='medium' className='w-full'>
            <Title heading={5}>{t('延长初始化')}</Title>
            <Text type='secondary'>
              {t('正在延长')} {dialog.records.length} {t('条返利记录')}
            </Text>
            <div>
              <Text strong>{t('初始化结束时间')}</Text>
              <Input
                className='mt-2'
                type='datetime-local'
                value={extendUntil}
                onChange={setExtendUntil}
              />
            </div>
            <Space className='justify-end'>
              <Button onClick={() => setDialog(null)}>{t('取消')}</Button>
              <Button type='primary' loading={mutating} onClick={submitExtend}>
                {t('延长')}
              </Button>
            </Space>
          </Space>
        )}
      </Modal>
    </Card>
  );
}

function InvitationRegistrationsPanel() {
  const { t } = useTranslation();
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  const [data, setData] = useState({ items: [], total: 0 });
  const [loading, setLoading] = useState(false);
  const [mutatingKey, setMutatingKey] = useState('');

  const loadRegistrations = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationAdminApi.getInvitationRegistrations({
        page,
        pageSize,
      });
      setData(extractData(response, { items: [], total: 0 }));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载邀请注册列表失败')));
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, t]);

  useEffect(() => {
    loadRegistrations();
  }, [loadRegistrations]);

  const renderReward = (generated, amount, status) => (
    <Space vertical spacing={2} align='start'>
      <span>{amount > 0 ? formatRebateAmount(amount) : '-'}</span>
      <Tag color={generated ? 'green' : 'grey'}>
        {generated ? requestStatusLabel(t, status) : t('未生成')}
      </Tag>
    </Space>
  );

  const generateReward = async (type, record) => {
    const key = `${type}:${record.id}`;
    setMutatingKey(key);
    try {
      const operation =
        type === 'inviter'
          ? invitationAdminApi.generateInvitationInviterReward
          : invitationAdminApi.generateInvitationInviteeReward;
      const response = await operation(record.id);
      const result = extractData(response, null);
      showSuccess(
        result?.generated
          ? type === 'inviter'
            ? t('邀请者奖励已生成')
            : t('被邀请者奖励已生成')
          : type === 'inviter'
            ? t('邀请者奖励已存在')
            : t('被邀请者奖励已存在'),
      );
      await loadRegistrations();
    } catch (error) {
      showError(
        invitationErrorMessage(
          error,
          type === 'inviter'
            ? t('生成邀请者奖励失败')
            : t('生成被邀请者奖励失败'),
        ),
      );
    } finally {
      setMutatingKey('');
    }
  };

  const columns = useMemo(
    () => [
      {
        title: t('邀请人'),
        dataIndex: 'inviterId',
        width: 180,
        render: (value, record) => formatUser(value, record.inviterName),
      },
      {
        title: t('被邀请用户'),
        dataIndex: 'inviteeId',
        width: 180,
        render: (value, record) => formatUser(value, record.inviteeName),
      },
      {
        title: t('用户组'),
        dataIndex: 'userGroup',
        width: 130,
        render: (value) => <Tag color='blue'>{groupNameLabel(t, value)}</Tag>,
      },
      {
        title: t('邀请时间'),
        dataIndex: 'invitedAt',
        width: 190,
        render: (value) => formatDateTime(value),
      },
      {
        title: t('累计返利'),
        dataIndex: 'totalRewardAmount',
        width: 140,
        render: (value) => formatRebateAmount(value),
      },
      {
        title: t('邀请者奖励'),
        dataIndex: 'inviterRewardAmount',
        width: 170,
        render: (value, record) =>
          renderReward(
            record.inviterRewardGenerated,
            value,
            record.inviterRewardStatus,
          ),
      },
      {
        title: t('被邀请者奖励'),
        dataIndex: 'inviteeRewardAmount',
        width: 170,
        render: (value, record) =>
          renderReward(
            record.inviteeRewardGenerated,
            value,
            record.inviteeRewardStatus,
          ),
      },
      {
        title: t('操作'),
        dataIndex: 'actions',
        fixed: 'right',
        width: 260,
        align: 'center',
        render: (_, record) => (
          <Space>
            <Button
              icon={<Gift size={14} />}
              theme='borderless'
              disabled={Boolean(mutatingKey) || record.inviterRewardGenerated}
              loading={mutatingKey === `inviter:${record.id}`}
              onClick={() => generateReward('inviter', record)}
            >
              {t('生成邀请者奖励')}
            </Button>
            <Button
              icon={<UserRoundPlus size={14} />}
              theme='borderless'
              disabled={Boolean(mutatingKey) || record.inviteeRewardGenerated}
              loading={mutatingKey === `invitee:${record.id}`}
              onClick={() => generateReward('invitee', record)}
            >
              {t('生成被邀请者奖励')}
            </Button>
          </Space>
        ),
      },
    ],
    [mutatingKey, t],
  );

  return (
    <Card title={t('邀请注册列表')} className='w-full min-w-0 !rounded-2xl'>
      <Table
        columns={columns}
        dataSource={data.items}
        loading={loading}
        pagination={false}
        rowKey='id'
        scroll={{ x: 1420 }}
        empty={<Empty description={t('暂无邀请注册记录')} />}
      />
      {data.total > 0 && (
        <div className='mt-4 flex flex-col gap-3 md:flex-row md:items-center md:justify-between'>
          <Text type='secondary'>
            {formatPageTotal(t, page, pageSize, data.total)}
          </Text>
          <Pagination
            currentPage={page}
            pageSize={pageSize}
            total={data.total}
            showSizeChanger
            pageSizeOpts={[10, 20, 50]}
            onPageChange={setPage}
            onPageSizeChange={(size) => {
              setPageSize(size);
              setPage(1);
            }}
          />
        </div>
      )}
    </Card>
  );
}

function ConfirmBatchAction({
  title,
  description,
  count,
  loading,
  okText,
  okType,
  onCancel,
  onOk,
}) {
  const { t } = useTranslation();
  return (
    <Space vertical spacing='medium' className='w-full'>
      <Title heading={5}>{title}</Title>
      <Text type='secondary'>
        {description} {t('数量：')}
        {count}
      </Text>
      <Space className='justify-end'>
        <Button onClick={onCancel}>{t('取消')}</Button>
        <Button type={okType || 'primary'} loading={loading} onClick={onOk}>
          {okText}
        </Button>
      </Space>
    </Space>
  );
}

function RebateRequestsPanel() {
  const { t } = useTranslation();
  const [statusFilter, setStatusFilter] = useState('all');
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  const [data, setData] = useState({ items: [], total: 0 });
  const [loading, setLoading] = useState(false);
  const [dialog, setDialog] = useState(null);
  const [note, setNote] = useState('');
  const [reason, setReason] = useState('');
  const [mutating, setMutating] = useState(false);

  const loadRequests = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationAdminApi.getRebateRequests({
        page,
        pageSize,
        ...(statusFilter !== 'all' ? { status: statusFilter } : {}),
      });
      setData(extractData(response, { items: [], total: 0 }));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载返利申请失败')));
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, statusFilter, t]);

  useEffect(() => {
    loadRequests();
  }, [loadRequests]);

  const openAction = (request, type) => {
    setDialog({ request, type });
    setNote(request.reviewNote || '');
    setReason(request.rejectReason || request.reviewNote || '');
  };

  const submitAction = async () => {
    if (!dialog?.request) return;
    if (dialog.type === 'reject' && !reason.trim()) {
      showError(t('请输入拒绝原因'));
      return;
    }

    setMutating(true);
    try {
      let response;
      if (dialog.type === 'approve') {
        response = await invitationAdminApi.approveRebateRequest(
          dialog.request.id,
          {
            note: note.trim() || undefined,
          },
        );
      } else if (dialog.type === 'reject') {
        response = await invitationAdminApi.rejectRebateRequest(
          dialog.request.id,
          {
            reason: reason.trim(),
            note: note.trim() || undefined,
          },
        );
      } else if (dialog.type === 'reset') {
        response = await invitationAdminApi.resetRebateRequestReview(
          dialog.request.id,
        );
      } else {
        response = await invitationAdminApi.completeRebateRequest(
          dialog.request.id,
        );
      }
      extractData(response, null);
      showSuccess(t('操作成功'));
      setDialog(null);
      await loadRequests();
    } catch (error) {
      showError(invitationErrorMessage(error, t('操作失败')));
    } finally {
      setMutating(false);
    }
  };

  const columns = useMemo(
    () => [
      {
        title: t('返利用户'),
        dataIndex: 'userId',
        render: (value, record) => formatUser(value, record.userName),
      },
      {
        title: t('返利金额'),
        dataIndex: 'amount',
        render: (value) => formatRebateAmount(value),
      },
      {
        title: t('状态'),
        dataIndex: 'status',
        render: (value) => {
          const colorMap = {
            pending: 'yellow',
            approved: 'green',
            rejected: 'red',
            completed: 'grey',
          };
          return (
            <Tag color={colorMap[value] || 'grey'}>
              {requestStatusLabel(t, value)}
            </Tag>
          );
        },
      },
      {
        title: t('创建时间'),
        dataIndex: 'createdAt',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('通过时间'),
        dataIndex: 'approvedAt',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('完成时间'),
        dataIndex: 'completedAt',
        render: (value) => formatDateTime(value),
      },
      {
        title: t('审核备注'),
        dataIndex: 'reviewNote',
        render: (value, record) => value || record.rejectReason || '-',
      },
      {
        title: t('操作'),
        dataIndex: 'actions',
        render: (_, record) => (
          <Space>
            {record.status !== 'completed' && (
              <>
                <Button
                  icon={<CheckCircle size={15} />}
                  theme='borderless'
                  onClick={() => openAction(record, 'approve')}
                />
                <Button
                  icon={<XCircle size={15} />}
                  theme='borderless'
                  type='danger'
                  onClick={() => openAction(record, 'reject')}
                />
                {record.status !== 'pending' && (
                  <Button
                    icon={<RotateCcw size={15} />}
                    theme='borderless'
                    onClick={() => openAction(record, 'reset')}
                  />
                )}
              </>
            )}
            {record.status === 'approved' && (
              <Button
                icon={<Check size={15} />}
                theme='borderless'
                onClick={() => openAction(record, 'complete')}
              />
            )}
            {record.status === 'completed' && '-'}
          </Space>
        ),
      },
    ],
    [t],
  );

  const dialogTitle = {
    approve: t('通过返利申请'),
    reject: t('拒绝返利申请'),
    reset: t('撤回审核'),
    complete: t('标记处理完成'),
  }[dialog?.type];

  return (
    <Card
      className='w-full min-w-0 !rounded-2xl'
      title={t('返利审批')}
      headerExtraContent={
        <Select
          value={statusFilter}
          onChange={(value) => {
            setStatusFilter(value);
            setPage(1);
          }}
          style={{ width: 150 }}
        >
          <Select.Option value='all'>{t('全部状态')}</Select.Option>
          <Select.Option value='pending'>{t('待处理')}</Select.Option>
          <Select.Option value='approved'>{t('已通过')}</Select.Option>
          <Select.Option value='rejected'>{t('已拒绝')}</Select.Option>
          <Select.Option value='completed'>{t('已完成')}</Select.Option>
        </Select>
      }
    >
      <Table
        columns={columns}
        dataSource={data.items}
        loading={loading}
        rowKey='id'
        pagination={false}
        empty={<Empty description={t('暂无返利申请')} />}
        scroll={{ x: 1120 }}
      />
      {data.total > 0 && (
        <div className='mt-4 flex flex-wrap items-center justify-between gap-3'>
          <Text type='secondary'>
            {formatPageTotal(t, page, pageSize, data.total)}
          </Text>
          <Pagination
            currentPage={page}
            pageSize={pageSize}
            total={data.total}
            showSizeChanger
            pageSizeOpts={[10, 20, 50]}
            onPageChange={setPage}
            onPageSizeChange={(size) => {
              setPageSize(size);
              setPage(1);
            }}
          />
        </div>
      )}

      <Modal
        title={dialogTitle}
        visible={Boolean(dialog)}
        onOk={submitAction}
        onCancel={() => setDialog(null)}
        okText={t('确认')}
        cancelText={t('取消')}
        confirmLoading={mutating}
      >
        <Space vertical spacing='medium' className='w-full'>
          {dialog?.request && (
            <Text type='secondary'>
              {formatUser(dialog.request.userId, dialog.request.userName)} ·{' '}
              {formatRebateAmount(dialog.request.amount)}
            </Text>
          )}
          {dialog?.type === 'reject' && (
            <div>
              <Text strong>{t('拒绝原因')}</Text>
              <TextArea
                className='mt-2'
                value={reason}
                onChange={setReason}
                autosize
              />
            </div>
          )}
          {dialog?.type !== 'complete' && dialog?.type !== 'reset' && (
            <div>
              <Text strong>{t('备注')}</Text>
              <TextArea
                className='mt-2'
                value={note}
                onChange={setNote}
                autosize
              />
            </div>
          )}
          {dialog?.type === 'reset' && (
            <Text type='secondary'>
              {t('撤回后申请会回到待审核状态，并清空当前审核备注。')}
            </Text>
          )}
        </Space>
      </Modal>
    </Card>
  );
}

function RebateStatsPanel() {
  const { t } = useTranslation();
  const [stats, setStats] = useState(null);
  const [loading, setLoading] = useState(false);
  const [userId, setUserId] = useState();
  const [userRecords, setUserRecords] = useState({ items: [], total: 0 });
  const [userLoading, setUserLoading] = useState(false);

  const loadStats = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationAdminApi.getRebateStats();
      setStats(extractData(response, null));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载返利统计失败')));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    loadStats();
  }, [loadStats]);

  const loadUserDetails = async () => {
    if (!userId) {
      showError(t('请输入用户 ID'));
      return;
    }
    setUserLoading(true);
    try {
      const response = await invitationAdminApi.getUserRebateDetails(userId);
      setUserRecords(extractData(response, { items: [], total: 0 }));
    } catch (error) {
      showError(invitationErrorMessage(error, t('加载用户返利详情失败')));
    } finally {
      setUserLoading(false);
    }
  };

  const statCards = [
    {
      title: t('累计返利'),
      value: formatRebateAmount(stats?.total_rebate || 0),
    },
    {
      title: t('已返利'),
      value: formatRebateAmount(stats?.completed_rebate || 0),
    },
    {
      title: t('待返利'),
      value: formatRebateAmount(stats?.pending_rebate || 0),
    },
    {
      title: t('已申请返利'),
      value: formatRebateAmount(stats?.requested_rebate || 0),
    },
    {
      title: t('已通过返利'),
      value: formatRebateAmount(stats?.approved_rebate || 0),
    },
    { title: t('邀请总数'), value: stats?.total_invitations || 0 },
  ];

  const userColumns = useMemo(
    () => [
      {
        title: t('订单类型'),
        dataIndex: 'orderType',
        render: (value) => orderTypeLabel(t, value),
      },
      {
        title: t('订单金额'),
        dataIndex: 'orderAmount',
        render: (value, record) =>
          isInvitationSignupReward(record.orderType)
            ? '-'
            : formatRebateAmount(value),
      },
      {
        title: t('返利金额'),
        dataIndex: 'rebateAmount',
        render: (value) => formatRebateAmount(value),
      },
      {
        title: t('状态'),
        dataIndex: 'displayStatus',
        render: (value, record) => (
          <Tag>{userRebateStatusLabel(t, value, record.status)}</Tag>
        ),
      },
      {
        title: t('创建时间'),
        dataIndex: 'createdAt',
        render: (value) => formatDateTime(value),
      },
    ],
    [t],
  );

  return (
    <div className='flex w-full min-w-0 flex-col gap-4'>
      <Card
        title={t('返利统计')}
        className='w-full min-w-0 !rounded-2xl'
        bordered
        style={{ width: '100%' }}
      >
        <Spin spinning={loading}>
          <Row gutter={[16, 16]}>
            {statCards.map((item) => (
              <Col xs={24} sm={12} lg={8} key={item.title}>
                <Card bodyStyle={{ padding: 16 }} className='!rounded-xl'>
                  <Text type='secondary'>{item.title}</Text>
                  <div className='mt-1 text-2xl font-bold'>{item.value}</div>
                </Card>
              </Col>
            ))}
          </Row>
        </Spin>
      </Card>

      <Card
        title={t('用户返利详情')}
        className='w-full min-w-0 !rounded-2xl'
        bordered
        style={{ width: '100%' }}
      >
        <div className='mb-4 flex flex-wrap items-end gap-2'>
          <div>
            <Text strong>{t('用户 ID')}</Text>
            <InputNumber
              className='mt-2'
              min={1}
              step={1}
              value={userId}
              onChange={setUserId}
              style={{ width: 220 }}
            />
          </div>
          <Button
            type='primary'
            loading={userLoading}
            onClick={loadUserDetails}
          >
            {t('查询')}
          </Button>
        </div>
        <Table
          columns={userColumns}
          dataSource={userRecords.items}
          loading={userLoading}
          rowKey='id'
          pagination={false}
          empty={<Empty description={t('暂无用户返利详情')} />}
          scroll={{ x: 860 }}
        />
      </Card>
    </div>
  );
}

const InvitationsAdmin = () => {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const { available, loading } = useInvitationBackendAvailable();
  const currentTab = searchParams.get('tab') || DEFAULT_TAB;

  if (loading || !available) {
    return null;
  }

  return (
    <div className='mt-[60px] px-2'>
      <div className='flex w-full min-w-0 flex-col gap-4 pb-8'>
        <div>
          <Title heading={3} style={{ marginBottom: 4 }}>
            {t('返利管理')}
          </Title>
          <Text type='secondary'>
            {t('管理返利规则、返利记录、邀请注册、返利审批和统计')}
          </Text>
        </div>

        <Tabs
          className='w-full min-w-0'
          style={{ width: '100%' }}
          type='button'
          activeKey={currentTab}
          onChange={(key) => setSearchParams({ tab: key })}
        >
          <TabPane
            tab={<TabLabel icon={Settings}>{t('返利规则')}</TabLabel>}
            itemKey='rules'
          >
            <div className='w-full min-w-0'>
              <RebateRulesPanel />
            </div>
          </TabPane>
          <TabPane
            tab={<TabLabel icon={ReceiptText}>{t('返利记录')}</TabLabel>}
            itemKey='records'
          >
            <div className='w-full min-w-0'>
              <RebateOrderRecordsPanel />
            </div>
          </TabPane>
          <TabPane
            tab={<TabLabel icon={UserRoundPlus}>{t('邀请注册')}</TabLabel>}
            itemKey='registrations'
          >
            <div className='w-full min-w-0'>
              <InvitationRegistrationsPanel />
            </div>
          </TabPane>
          <TabPane
            tab={<TabLabel icon={CheckCircle}>{t('返利审批')}</TabLabel>}
            itemKey='approvals'
          >
            <div className='w-full min-w-0'>
              <RebateRequestsPanel />
            </div>
          </TabPane>
          <TabPane
            tab={<TabLabel icon={BarChart3}>{t('统计')}</TabLabel>}
            itemKey='statistics'
          >
            <div className='w-full min-w-0'>
              <RebateStatsPanel />
            </div>
          </TabPane>
        </Tabs>
      </div>
    </div>
  );
};

export default InvitationsAdmin;
