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

import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  Banner,
  Button,
  Card,
  Empty,
  Modal,
  Progress,
  Space,
  Spin,
  Table,
  TabPane,
  Tabs,
  Tag,
  Tooltip,
  Typography,
} from '@douyinfe/semi-ui';
import {
  Activity,
  Clock3,
  ListChecks,
  RefreshCw,
  ServerCog,
  Trash2,
} from 'lucide-react';
import { API, showError, showSuccess } from '../../helpers';

const { Text, Title } = Typography;
const INSTANCE_POLL_INTERVAL = 30000;
const TASK_POLL_INTERVAL = 8000;

const TASK_TYPE_LABELS = {
  log_cleanup: '日志清理',
  channel_test: '批量渠道测试',
  model_update: '批量上游模型更新',
  midjourney_poll: '绘图任务轮询',
  async_task_poll: '异步任务轮询',
};

const STATUS_COLORS = {
  online: 'green',
  stale: 'orange',
  pending: 'orange',
  running: 'blue',
  succeeded: 'green',
  failed: 'red',
};

const isActiveTask = (task) =>
  task?.status === 'pending' || task?.status === 'running';

const toDate = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return null;
  return new Date(numeric < 1000000000000 ? numeric * 1000 : numeric);
};

const formatTimestamp = (value) => {
  const date = toDate(value);
  return date ? date.toLocaleString() : '-';
};

const formatPercent = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return '-';
  return `${numeric.toFixed(1)}%`;
};

const getResourcePercent = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return null;
  return Math.max(0, Math.min(100, numeric));
};

const ResourceUsage = ({ value }) => {
  const percent = getResourcePercent(value);
  return (
    <div className='flex min-w-[96px] items-center gap-2'>
      <Progress
        percent={percent ?? 0}
        showInfo={false}
        size='small'
        style={{ width: 58 }}
        stroke={
          percent === null
            ? 'var(--semi-color-disabled-text)'
            : percent >= 90
              ? 'var(--semi-color-danger)'
              : percent >= 70
                ? 'var(--semi-color-warning)'
                : 'var(--semi-color-success)'
        }
      />
      <span className='text-xs tabular-nums'>{formatPercent(value)}</span>
    </div>
  );
};

const SummaryItem = ({ icon, label, value, tone = 'primary' }) => (
  <div className='flex min-w-0 items-center gap-3 px-4 py-3'>
    <span
      className='flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-semi-color-fill-0'
      style={{ color: `var(--semi-color-${tone})` }}
    >
      {icon}
    </span>
    <div className='min-w-0'>
      <div className='truncate text-xs text-semi-color-text-2'>{label}</div>
      <div className='truncate text-lg font-semibold text-semi-color-text-0'>
        {value}
      </div>
    </div>
  </div>
);

const SystemInfo = () => {
  const { t } = useTranslation();
  const mountedRef = useRef(true);
  const [instances, setInstances] = useState([]);
  const [tasks, setTasks] = useState([]);
  const [instancesLoading, setInstancesLoading] = useState(true);
  const [tasksLoading, setTasksLoading] = useState(true);
  const [instancesRefreshing, setInstancesRefreshing] = useState(false);
  const [tasksRefreshing, setTasksRefreshing] = useState(false);
  const [instancesError, setInstancesError] = useState('');
  const [tasksError, setTasksError] = useState('');
  const [deletingNode, setDeletingNode] = useState('');
  const [deletingAll, setDeletingAll] = useState(false);

  const loadInstances = useCallback(
    async ({ background = false } = {}) => {
      if (background) setInstancesRefreshing(true);
      else setInstancesLoading(true);
      setInstancesError('');
      try {
        const res = await API.get('/api/system-info/instances', {
          skipErrorHandler: true,
        });
        if (!res?.data?.success || !Array.isArray(res?.data?.data)) {
          throw new Error(res?.data?.message || t('实例信息加载失败'));
        }
        if (mountedRef.current) setInstances(res.data.data);
      } catch (error) {
        if (mountedRef.current) {
          setInstancesError(
            error?.response?.data?.message ||
              error?.message ||
              t('实例信息加载失败'),
          );
        }
      } finally {
        if (mountedRef.current) {
          setInstancesLoading(false);
          setInstancesRefreshing(false);
        }
      }
    },
    [t],
  );

  const loadTasks = useCallback(
    async ({ background = false } = {}) => {
      if (background) setTasksRefreshing(true);
      else setTasksLoading(true);
      setTasksError('');
      try {
        const res = await API.get('/api/system-task/list', {
          params: { limit: 20 },
          skipErrorHandler: true,
        });
        if (!res?.data?.success || !Array.isArray(res?.data?.data)) {
          throw new Error(res?.data?.message || t('系统任务加载失败'));
        }
        if (mountedRef.current) setTasks(res.data.data);
      } catch (error) {
        if (mountedRef.current) {
          setTasksError(
            error?.response?.data?.message ||
              error?.message ||
              t('系统任务加载失败'),
          );
        }
      } finally {
        if (mountedRef.current) {
          setTasksLoading(false);
          setTasksRefreshing(false);
        }
      }
    },
    [t],
  );

  useEffect(() => {
    mountedRef.current = true;
    loadInstances();
    loadTasks();
    const interval = window.setInterval(
      () => loadInstances({ background: true }),
      INSTANCE_POLL_INTERVAL,
    );
    return () => {
      mountedRef.current = false;
      window.clearInterval(interval);
    };
  }, [loadInstances, loadTasks]);

  const activeTasks = useMemo(() => tasks.filter(isActiveTask), [tasks]);
  const historyTasks = useMemo(
    () => tasks.filter((task) => !isActiveTask(task)),
    [tasks],
  );

  useEffect(() => {
    if (activeTasks.length === 0) return undefined;
    const interval = window.setInterval(
      () => loadTasks({ background: true }),
      TASK_POLL_INTERVAL,
    );
    return () => window.clearInterval(interval);
  }, [activeTasks.length, loadTasks]);

  const staleInstances = useMemo(
    () => instances.filter((instance) => instance.status === 'stale'),
    [instances],
  );
  const onlineInstances = instances.length - staleInstances.length;

  const deleteInstance = async (instance) => {
    setDeletingNode(instance.node_name);
    try {
      const res = await API.delete(
        `/api/system-info/instances/${encodeURIComponent(instance.node_name)}`,
        { skipErrorHandler: true },
      );
      if (!res?.data?.success) {
        throw new Error(res?.data?.message || t('删除失败'));
      }
      showSuccess(t('已删除失联实例'));
      await loadInstances({ background: true });
    } catch (error) {
      showError(
        error?.response?.data?.message || error?.message || t('删除失败'),
      );
      await loadInstances({ background: true });
    } finally {
      if (mountedRef.current) setDeletingNode('');
    }
  };

  const confirmDeleteInstance = (instance) => {
    Modal.confirm({
      title: t('删除失联实例'),
      content: t('确定删除实例 {{name}} 的失联记录吗？', {
        name: instance.info?.node?.name || instance.node_name,
      }),
      okText: t('删除'),
      cancelText: t('取消'),
      okType: 'danger',
      onOk: () => deleteInstance(instance),
    });
  };

  const deleteAllStale = async () => {
    setDeletingAll(true);
    try {
      const res = await API.delete('/api/system-info/stale-instances', {
        skipErrorHandler: true,
      });
      if (!res?.data?.success) {
        throw new Error(res?.data?.message || t('删除失败'));
      }
      showSuccess(
        t('已删除 {{count}} 个失联实例', {
          count: res?.data?.data?.deleted_count || 0,
        }),
      );
      await loadInstances({ background: true });
    } catch (error) {
      showError(
        error?.response?.data?.message || error?.message || t('删除失败'),
      );
    } finally {
      if (mountedRef.current) setDeletingAll(false);
    }
  };

  const confirmDeleteAllStale = () => {
    Modal.confirm({
      title: t('清理全部失联实例'),
      content: t('将删除 {{count}} 个失联实例记录，在线实例不会受影响。', {
        count: staleInstances.length,
      }),
      okText: t('全部清理'),
      cancelText: t('取消'),
      okType: 'danger',
      onOk: deleteAllStale,
    });
  };

  const instanceColumns = useMemo(
    () => [
      {
        title: t('实例'),
        dataIndex: 'node_name',
        width: 230,
        render: (_, instance) => (
          <div className='min-w-0'>
            <div className='truncate text-sm font-medium'>
              {instance.info?.node?.name || instance.node_name}
            </div>
            <div className='truncate font-mono text-xs text-semi-color-text-2'>
              {instance.info?.host?.hostname || instance.node_name}
            </div>
          </div>
        ),
      },
      {
        title: t('状态'),
        dataIndex: 'status',
        width: 90,
        render: (value) => (
          <Tag color={STATUS_COLORS[value] || 'grey'}>{t(value)}</Tag>
        ),
      },
      {
        title: t('角色'),
        width: 90,
        render: (_, instance) => (
          <Tag color={instance.info?.role?.is_master ? 'purple' : 'grey'}>
            {instance.info?.role?.is_master ? 'master' : 'worker'}
          </Tag>
        ),
      },
      {
        title: 'CPU',
        width: 120,
        render: (_, instance) => (
          <ResourceUsage value={instance.info?.resources?.cpu?.usage_percent} />
        ),
      },
      {
        title: t('内存'),
        width: 120,
        render: (_, instance) => (
          <ResourceUsage
            value={instance.info?.resources?.memory?.usage_percent}
          />
        ),
      },
      {
        title: t('存储'),
        width: 120,
        render: (_, instance) => (
          <ResourceUsage
            value={instance.info?.resources?.storage?.used_percent}
          />
        ),
      },
      {
        title: t('版本'),
        width: 120,
        render: (_, instance) => (
          <span className='font-mono text-xs'>
            {instance.info?.runtime?.version || '-'}
          </span>
        ),
      },
      {
        title: t('运行环境'),
        width: 130,
        render: (_, instance) => {
          const runtime = instance.info?.runtime || {};
          return (
            <span className='font-mono text-xs'>
              {[runtime.goos, runtime.goarch].filter(Boolean).join('/') || '-'}
            </span>
          );
        },
      },
      {
        title: t('最后心跳'),
        dataIndex: 'last_seen_at',
        width: 180,
        render: formatTimestamp,
      },
      {
        title: '',
        width: 64,
        fixed: 'right',
        render: (_, instance) =>
          instance.status === 'stale' ? (
            <Tooltip content={t('删除失联实例')}>
              <Button
                icon={<Trash2 size={14} />}
                type='danger'
                theme='borderless'
                loading={deletingNode === instance.node_name}
                disabled={Boolean(deletingNode) || deletingAll}
                onClick={() => confirmDeleteInstance(instance)}
              />
            </Tooltip>
          ) : (
            '-'
          ),
      },
    ],
    [deletingAll, deletingNode, t],
  );

  const taskColumns = useMemo(
    () => [
      {
        title: t('任务类型'),
        dataIndex: 'type',
        width: 230,
        render: (value) => (
          <div>
            <div className='text-sm font-medium'>
              {t(TASK_TYPE_LABELS[value] || value)}
            </div>
            <div className='font-mono text-xs text-semi-color-text-2'>
              {value}
            </div>
          </div>
        ),
      },
      {
        title: t('状态'),
        dataIndex: 'status',
        width: 100,
        render: (value) => (
          <Tag color={STATUS_COLORS[value] || 'grey'}>{t(value)}</Tag>
        ),
      },
      {
        title: t('进度'),
        width: 160,
        render: (_, task) => {
          const progress = getResourcePercent(task.state?.progress);
          return (
            <div className='flex items-center gap-2'>
              <Progress
                percent={progress ?? 0}
                showInfo={false}
                size='small'
                style={{ width: 96 }}
              />
              <span className='w-10 text-right text-xs tabular-nums'>
                {progress === null ? '-' : `${progress}%`}
              </span>
            </div>
          );
        },
      },
      {
        title: t('执行节点'),
        dataIndex: 'locked_by',
        width: 220,
        render: (value) => (
          <span className='font-mono text-xs'>{value || '-'}</span>
        ),
      },
      {
        title: t('更新时间'),
        dataIndex: 'updated_at',
        width: 180,
        render: formatTimestamp,
      },
      {
        title: t('详情'),
        dataIndex: 'error',
        width: 260,
        render: (value) => (
          <Text
            type={value ? 'danger' : 'tertiary'}
            ellipsis={{ showTooltip: true }}
          >
            {value || '-'}
          </Text>
        ),
      },
    ],
    [t],
  );

  const taskTable = (rows) =>
    rows.length === 0 && !tasksLoading ? (
      <div className='py-10'>
        <Empty title={t('暂无系统任务')} />
      </div>
    ) : (
      <Table
        columns={taskColumns}
        dataSource={rows}
        rowKey='task_id'
        pagination={false}
        size='small'
        scroll={{ x: 1000 }}
      />
    );

  return (
    <main className='w-full'>
      <div className='mx-auto w-full max-w-[1440px]'>
        <div className='mb-5 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between'>
          <div>
            <div className='mb-2 flex items-center gap-2 text-semi-color-primary'>
              <ServerCog size={18} />
              <Text strong type='secondary'>
                {t('集群运行状态')}
              </Text>
            </div>
            <Title heading={3} style={{ margin: 0 }}>
              {t('系统信息')}
            </Title>
            <Text type='tertiary' className='mt-1 block'>
              {t('查看实例心跳、资源使用情况和后台系统任务。')}
            </Text>
          </div>
          <Space wrap>
            <Text type='tertiary' size='small'>
              {activeTasks.length > 0
                ? t('运行中任务每 {{seconds}} 秒自动刷新', {
                    seconds: TASK_POLL_INTERVAL / 1000,
                  })
                : t('当前没有运行中的系统任务')}
            </Text>
            <Button
              icon={<RefreshCw size={15} />}
              type='tertiary'
              theme='outline'
              loading={instancesRefreshing || tasksRefreshing}
              onClick={() => {
                loadInstances({ background: true });
                loadTasks({ background: true });
              }}
            >
              {t('刷新')}
            </Button>
          </Space>
        </div>

        <div className='mb-5 grid grid-cols-1 divide-y divide-semi-color-border border-y border-semi-color-border sm:grid-cols-3 sm:divide-x sm:divide-y-0'>
          <SummaryItem
            icon={<Activity size={18} />}
            label={t('在线实例')}
            value={onlineInstances}
            tone='success'
          />
          <SummaryItem
            icon={<Clock3 size={18} />}
            label={t('失联实例')}
            value={staleInstances.length}
            tone='warning'
          />
          <SummaryItem
            icon={<ListChecks size={18} />}
            label={t('运行中任务')}
            value={activeTasks.length}
          />
        </div>

        <div className='grid grid-cols-1 gap-5'>
          <Card
            title={
              <Space>
                <ServerCog size={16} />
                <span>{t('实例')}</span>
              </Space>
            }
            headerExtraContent={
              <Space>
                <Text type='tertiary' size='small'>
                  {t('每 {{seconds}} 秒自动刷新', {
                    seconds: INSTANCE_POLL_INTERVAL / 1000,
                  })}
                </Text>
                <Button
                  icon={<Trash2 size={14} />}
                  type='danger'
                  theme='outline'
                  loading={deletingAll}
                  disabled={
                    staleInstances.length === 0 || Boolean(deletingNode)
                  }
                  onClick={confirmDeleteAllStale}
                >
                  {t('清理全部失联实例')}
                </Button>
              </Space>
            }
            bodyStyle={{ padding: 0 }}
          >
            {instancesError ? (
              <Banner
                type='danger'
                closeIcon={null}
                description={instancesError}
              />
            ) : null}
            <Spin spinning={instancesLoading} tip={t('加载中...')}>
              {instances.length === 0 && !instancesLoading ? (
                <div className='py-10'>
                  <Empty title={t('暂无实例上报')} />
                </div>
              ) : (
                <Table
                  columns={instanceColumns}
                  dataSource={instances}
                  rowKey='node_name'
                  pagination={false}
                  size='small'
                  scroll={{ x: 1200 }}
                />
              )}
            </Spin>
          </Card>

          <Card
            title={
              <Space>
                <ListChecks size={16} />
                <span>{t('系统任务')}</span>
              </Space>
            }
            bodyStyle={{ padding: 0 }}
          >
            {tasksError ? (
              <Banner type='danger' closeIcon={null} description={tasksError} />
            ) : null}
            <Spin spinning={tasksLoading} tip={t('加载中...')}>
              <Tabs type='line'>
                <TabPane
                  tab={`${t('运行中')} (${activeTasks.length})`}
                  itemKey='active'
                >
                  {taskTable(activeTasks)}
                </TabPane>
                <TabPane
                  tab={`${t('任务历史')} (${historyTasks.length})`}
                  itemKey='history'
                >
                  {taskTable(historyTasks)}
                </TabPane>
              </Tabs>
            </Spin>
          </Card>
        </div>
      </div>
    </main>
  );
};

export default SystemInfo;
