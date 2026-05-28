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
import { Navigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Card,
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
  Table,
  TabPane,
  Tabs,
  Tag,
  Typography,
} from '@douyinfe/semi-ui';
import {
  IconCopy,
  IconGift,
  IconHistory,
  IconQrCode,
} from '@douyinfe/semi-icons';
import { Wallet } from 'lucide-react';
import { QRCodeSVG } from 'qrcode.react';
import { copy, showError, showSuccess } from '../../helpers';
import { invitationApi } from '../../components/invitations/api';
import {
  DEFAULT_PAGE_SIZE,
  displayAmountToCents,
  extractData,
  formatDateTime,
  formatPercent,
  formatRebateAmount,
  isInvitationSignupReward,
  orderTypeLabel,
  requestStatusLabel,
  userRebateStatusLabel,
} from '../../components/invitations/utils';

const { Text, Title } = Typography;

const DEFAULT_TAB = 'invite';

function getFeatureVisibility(status) {
  const userVisible =
    Boolean(status?.available) && Boolean(status?.userInvitationRebateEnabled);
  const hasAnyRebateFeature =
    Boolean(status?.orderRebateEnabled) ||
    Boolean(status?.invitationSignupRewardEnabled) ||
    Boolean(status?.rebateToBalanceEnabled);
  const rebateRecordsVisible =
    userVisible &&
    (Boolean(status?.orderRebateEnabled) ||
      Boolean(status?.invitationSignupRewardEnabled));
  const rebateManagementVisible =
    userVisible &&
    Boolean(status?.rebateToBalanceEnabled) &&
    (Boolean(status?.orderRebateEnabled) ||
      Boolean(status?.invitationSignupRewardEnabled));

  return {
    userVisible,
    hasAnyRebateFeature,
    rebateRecordsVisible,
    rebateManagementVisible,
  };
}

function formatPageTotal(t, page, pageSize, total) {
  if (!total) return '';
  const start = (page - 1) * pageSize + 1;
  const end = Math.min(page * pageSize, total);
  return `${t('显示第')} ${start} ${t('条 - 第')} ${end} ${t('条，共')} ${total} ${t('条')}`;
}

function useInvitationFeatureStatus() {
  const [featureStatus, setFeatureStatus] = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;

    async function loadStatus() {
      try {
        const response = await invitationApi.getStatus();
        if (!active) return;

        if (!response?.success || !response?.data?.available) {
          setFeatureStatus({ available: false });
          return;
        }

        setFeatureStatus(response.data);
      } catch (error) {
        if (active) setFeatureStatus({ available: false });
      } finally {
        if (active) setLoading(false);
      }
    }

    loadStatus();

    return () => {
      active = false;
    };
  }, []);

  return { featureStatus, loading, ...getFeatureVisibility(featureStatus) };
}

function StatCard({ title, value, muted }) {
  return (
    <Card bodyStyle={{ padding: 16 }} className='!rounded-xl'>
      <Text type='secondary'>{title}</Text>
      <div
        style={{
          marginTop: 6,
          fontSize: 24,
          fontWeight: 700,
          color: muted
            ? 'var(--semi-color-text-2)'
            : 'var(--semi-color-text-0)',
        }}
      >
        {value}
      </div>
    </Card>
  );
}

function InvitationCodeCard({ stats, loading, showRebateStats }) {
  const { t } = useTranslation();
  const [qrVisible, setQrVisible] = useState(false);

  const invitationLink = useMemo(() => {
    if (!stats?.invitationCode) return '';
    return `${window.location.origin}/?aff=${stats.invitationCode}`;
  }, [stats?.invitationCode]);

  const handleCopy = useCallback(
    async (value, successText) => {
      if (!value) return;
      const ok = await copy(value);
      if (ok) {
        showSuccess(successText);
      } else {
        showError(t('复制失败'));
      }
    },
    [t],
  );

  return (
    <>
      <Card
        title={showRebateStats ? t('我的邀请码') : t('我的邀请')}
        headerExtraContent={
          showRebateStats && stats?.invitationCode ? (
            <Button
              icon={<IconQrCode />}
              theme='outline'
              onClick={() => setQrVisible(true)}
            >
              {t('二维码')}
            </Button>
          ) : null
        }
        className='w-full min-w-0 !rounded-2xl'
      >
        <Spin spinning={loading}>
          {!stats ? (
            <Empty description={loading ? t('加载中') : t('暂无邀请数据')} />
          ) : (
            <Space vertical align='start' spacing='loose' className='w-full'>
              {showRebateStats && (
                <Row gutter={[16, 16]} className='w-full'>
                  <Col xs={24} md={10}>
                    <Text strong>{t('邀请码')}</Text>
                    <div className='mt-2 flex gap-2'>
                      <div
                        className='flex-1 rounded-xl px-4 py-3 font-mono text-2xl font-bold'
                        style={{
                          border: '1px solid var(--semi-color-border)',
                          background: 'var(--semi-color-fill-0)',
                          letterSpacing: 1,
                        }}
                      >
                        {stats.invitationCode}
                      </div>
                      <Button
                        icon={<IconCopy />}
                        theme='outline'
                        onClick={() =>
                          handleCopy(stats.invitationCode, t('邀请码已复制'))
                        }
                      />
                    </div>
                  </Col>
                  <Col xs={24} md={14}>
                    <Text strong>{t('邀请链接')}</Text>
                    <div className='mt-2 flex gap-2'>
                      <Input
                        value={invitationLink}
                        readOnly
                        className='font-mono'
                      />
                      <Button
                        icon={<IconCopy />}
                        theme='outline'
                        onClick={() =>
                          handleCopy(invitationLink, t('邀请链接已复制'))
                        }
                      />
                    </div>
                  </Col>
                </Row>
              )}

              <Row gutter={[16, 16]} className='w-full'>
                <Col xs={24} sm={showRebateStats ? 12 : 24} lg={6}>
                  <StatCard
                    title={t('已邀请人数')}
                    value={stats.invitedCount}
                  />
                </Col>
                {showRebateStats && (
                  <>
                    <Col xs={24} sm={12} lg={6}>
                      <StatCard
                        title={t('累计返利')}
                        value={formatRebateAmount(stats.totalRebate)}
                      />
                    </Col>
                    <Col xs={24} sm={12} lg={6}>
                      <StatCard
                        title={t('已返利')}
                        value={formatRebateAmount(stats.completedRebate)}
                      />
                    </Col>
                    <Col xs={24} sm={12} lg={6}>
                      <StatCard
                        title={t('待返利')}
                        value={formatRebateAmount(stats.pendingRebate)}
                      />
                    </Col>
                  </>
                )}
              </Row>
            </Space>
          )}
        </Spin>
      </Card>

      <Modal
        title={t('邀请二维码')}
        visible={qrVisible}
        onCancel={() => setQrVisible(false)}
        footer={null}
        width={360}
      >
        <div className='flex justify-center py-4'>
          <div
            style={{
              background: '#fff',
              borderRadius: 12,
              padding: 16,
              border: '1px solid var(--semi-color-border)',
            }}
          >
            <QRCodeSVG value={invitationLink} size={220} level='H' />
          </div>
        </div>
      </Modal>
    </>
  );
}

function RebateRecordsPanel() {
  const { t } = useTranslation();
  const [statusFilter, setStatusFilter] = useState('all');
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  const [data, setData] = useState({ items: [], total: 0 });
  const [loading, setLoading] = useState(false);

  const loadRecords = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationApi.getRebateRecords({
        page,
        pageSize,
        ...(statusFilter !== 'all' ? { status: statusFilter } : {}),
      });
      setData(extractData(response, { items: [], total: 0 }));
    } catch (error) {
      showError(error.message || t('加载返利记录失败'));
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, statusFilter, t]);

  useEffect(() => {
    loadRecords();
  }, [loadRecords]);

  const columns = useMemo(
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
        title: t('返利比例'),
        dataIndex: 'rebateRatio',
        render: (value, record) => {
          if (isInvitationSignupReward(record.orderType)) return '-';
          return value == null ? t('未配置') : formatPercent(value, 1);
        },
      },
      {
        title: t('状态'),
        dataIndex: 'displayStatus',
        render: (value, record) => {
          const status =
            value || (record.status === 'pending' ? 'claimable' : 'paid');
          const colorMap = {
            estimated: 'yellow',
            claimable: 'green',
            paid: 'grey',
            waiting_unlock: 'blue',
          };
          return (
            <Tag color={colorMap[status] || 'grey'} size='large'>
              {userRebateStatusLabel(t, value, record.status)}
            </Tag>
          );
        },
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
    <Card
      title={t('返利记录')}
      headerExtraContent={
        <Select
          value={statusFilter}
          onChange={(value) => {
            setStatusFilter(value);
            setPage(1);
          }}
          style={{ width: 160 }}
        >
          <Select.Option value='all'>{t('全部状态')}</Select.Option>
          <Select.Option value='pending'>{t('待处理')}</Select.Option>
          <Select.Option value='requested'>{t('已申请')}</Select.Option>
          <Select.Option value='approved'>{t('已通过')}</Select.Option>
          <Select.Option value='completed'>{t('已完成')}</Select.Option>
        </Select>
      }
      className='w-full min-w-0 !rounded-2xl'
    >
      <Table
        columns={columns}
        dataSource={data.items}
        loading={loading}
        rowKey='id'
        pagination={false}
        empty={<Empty description={t('暂无返利记录')} />}
        scroll={{ x: 920 }}
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
    </Card>
  );
}

function RebateBalancePanel() {
  const { t } = useTranslation();
  const [available, setAvailable] = useState({ amount: 0, recordIds: [] });
  const [requests, setRequests] = useState({ items: [], total: 0 });
  const [amount, setAmount] = useState();
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [loading, setLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const loadAvailable = useCallback(async () => {
    const response = await invitationApi.getAvailableRebates();
    setAvailable(extractData(response, { amount: 0, recordIds: [] }));
  }, []);

  const loadRequests = useCallback(async () => {
    setLoading(true);
    try {
      const response = await invitationApi.getMyRebateRequests({
        page,
        pageSize,
      });
      setRequests(extractData(response, { items: [], total: 0 }));
    } catch (error) {
      showError(error.message || t('加载返利到余额记录失败'));
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, t]);

  const refresh = useCallback(async () => {
    try {
      await Promise.all([loadAvailable(), loadRequests()]);
    } catch (error) {
      showError(error.message || t('加载返利到余额数据失败'));
    }
  }, [loadAvailable, loadRequests, t]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const submitRequest = async () => {
    const cents = displayAmountToCents(amount);
    if (!cents || cents <= 0) {
      showError(t('请输入有效返利金额'));
      return;
    }
    if (cents > available.amount) {
      showError(t('申请金额不能超过可返利金额'));
      return;
    }
    if (!available.recordIds || available.recordIds.length === 0) {
      showError(t('暂无可申请返利'));
      return;
    }

    setSubmitting(true);
    try {
      const response = await invitationApi.requestRebate({
        amount: cents,
        rebateRecordIds: available.recordIds,
      });
      extractData(response, null);
      showSuccess(t('返利到余额申请已提交'));
      setAmount();
      await refresh();
    } catch (error) {
      showError(error.message || t('提交返利到余额申请失败'));
    } finally {
      setSubmitting(false);
    }
  };

  const columns = useMemo(
    () => [
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
        title: t('拒绝原因'),
        dataIndex: 'rejectReason',
        render: (value) => value || '-',
      },
    ],
    [t],
  );

  return (
    <div className='flex w-full min-w-0 flex-col gap-4'>
      <Card
        title={t('返利到余额')}
        className='w-full min-w-0 !rounded-2xl'
        bordered
        style={{ width: '100%' }}
      >
        <Row gutter={[16, 16]} align='bottom'>
          <Col xs={24} md={8}>
            <Text type='secondary'>{t('可返利金额')}</Text>
            <div className='mt-1 text-2xl font-bold'>
              {formatRebateAmount(available.amount)}
            </div>
          </Col>
          <Col xs={24} md={10}>
            <Text strong>{t('申请金额')}</Text>
            <InputNumber
              value={amount}
              onChange={setAmount}
              min={0}
              step={0.01}
              precision={2}
              placeholder='0.00'
              className='mt-2 w-full'
            />
          </Col>
          <Col xs={24} md={6}>
            <Button
              type='primary'
              loading={submitting}
              disabled={!available.amount || !available.recordIds?.length}
              onClick={submitRequest}
              block
            >
              {t('申请到余额')}
            </Button>
          </Col>
        </Row>
      </Card>

      <Card
        title={t('返利到余额记录')}
        className='w-full min-w-0 !rounded-2xl'
        bordered
        style={{ width: '100%' }}
      >
        <Table
          columns={columns}
          dataSource={requests.items}
          loading={loading}
          rowKey='id'
          pagination={false}
          empty={<Empty description={t('暂无返利到余额记录')} />}
          scroll={{ x: 980 }}
        />
        {requests.total > 0 && (
          <div className='mt-4 flex flex-wrap items-center justify-between gap-3'>
            <Text type='secondary'>
              {formatPageTotal(t, page, pageSize, requests.total)}
            </Text>
            <Pagination
              currentPage={page}
              pageSize={pageSize}
              total={requests.total}
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
    </div>
  );
}

const Invitations = () => {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const invitationFeature = useInvitationFeatureStatus();
  const [stats, setStats] = useState(null);
  const [statsLoading, setStatsLoading] = useState(false);

  const activeTabFromUrl = searchParams.get('tab') || DEFAULT_TAB;
  const activeTab =
    (activeTabFromUrl === 'records' &&
      !invitationFeature.rebateRecordsVisible) ||
    (activeTabFromUrl === 'rebate' &&
      !invitationFeature.rebateManagementVisible)
      ? DEFAULT_TAB
      : activeTabFromUrl;

  useEffect(() => {
    let active = true;

    async function loadStats() {
      if (!invitationFeature.userVisible) return;
      setStatsLoading(true);
      try {
        const response = await invitationApi.getMyCode();
        if (active) setStats(extractData(response, null));
      } catch (error) {
        if (active) showError(error.message || t('加载邀请信息失败'));
      } finally {
        if (active) setStatsLoading(false);
      }
    }

    loadStats();

    return () => {
      active = false;
    };
  }, [invitationFeature.userVisible, t]);

  if (invitationFeature.loading) {
    return null;
  }

  if (!invitationFeature.userVisible) {
    return <Navigate to='/console/topup' replace />;
  }

  return (
    <div className='mt-[60px] px-2'>
      <div className='flex w-full min-w-0 flex-col gap-4 pb-8'>
        <div>
          <Title heading={3} style={{ marginBottom: 4 }}>
            {invitationFeature.hasAnyRebateFeature
              ? t('邀请返利')
              : t('我的邀请')}
          </Title>
          <Text type='secondary'>
            {invitationFeature.hasAnyRebateFeature
              ? t('邀请好友并查看返利记录')
              : t('查看你的邀请人数')}
          </Text>
        </div>

        <Tabs
          className='w-full min-w-0'
          style={{ width: '100%' }}
          type='button'
          activeKey={activeTab}
          onChange={(key) => setSearchParams({ tab: key })}
        >
          <TabPane
            tab={
              <span className='inline-flex items-center gap-1'>
                <IconGift />
                {t('我的邀请')}
              </span>
            }
            itemKey='invite'
          >
            <div className='w-full min-w-0'>
              <InvitationCodeCard
                stats={stats}
                loading={statsLoading}
                showRebateStats={invitationFeature.hasAnyRebateFeature}
              />
            </div>
          </TabPane>

          {invitationFeature.rebateRecordsVisible && (
            <TabPane
              tab={
                <span className='inline-flex items-center gap-1'>
                  <IconHistory />
                  {t('返利记录')}
                </span>
              }
              itemKey='records'
            >
              <div className='w-full min-w-0'>
                <RebateRecordsPanel />
              </div>
            </TabPane>
          )}

          {invitationFeature.rebateManagementVisible && (
            <TabPane
              tab={
                <span className='inline-flex items-center gap-1'>
                  <Wallet size={15} />
                  {t('返利管理')}
                </span>
              }
              itemKey='rebate'
            >
              <div className='w-full min-w-0'>
                <RebateBalancePanel />
              </div>
            </TabPane>
          )}
        </Tabs>
      </div>
    </div>
  );
};

export default Invitations;
