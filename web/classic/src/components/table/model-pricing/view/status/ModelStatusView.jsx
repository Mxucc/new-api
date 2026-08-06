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
  Empty,
  Input,
  Radio,
  RadioGroup,
  Spin,
  Table,
  Tag,
  Typography,
} from '@douyinfe/semi-ui';
import { VChart } from '@visactor/react-vchart';
import {
  Activity,
  Boxes,
  ChevronRight,
  CircleHelp,
  HeartPulse,
  Info,
  RefreshCw,
  Search,
  Timer,
} from 'lucide-react';
import { API, getLobeHubIcon } from '../../../../../helpers';
import { normalizeLanguage } from '../../../../../i18n/language';

const { Text, Title } = Typography;
const STATUS_SCOPE_RECENT = 'recent';
const STATUS_SCOPE_ALL = 'all';
const STATUS_BAR_SLOTS = ['oldest', 'middle', 'latest'];

const formatLatency = (value) => {
  const milliseconds = Number(value);
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return '-';
  if (milliseconds >= 1000) return `${(milliseconds / 1000).toFixed(2)}s`;
  return `${Math.round(milliseconds)}ms`;
};

const formatThroughput = (value) => {
  const throughput = Number(value);
  if (!Number.isFinite(throughput) || throughput <= 0) return '-';
  if (throughput >= 1000) return `${(throughput / 1000).toFixed(1)}K t/s`;
  return `${throughput.toFixed(throughput < 10 ? 2 : 1)} t/s`;
};

const formatRequestCount = (value) => {
  const count = Number(value);
  if (!Number.isFinite(count) || count < 0) return '-';
  return Math.round(count).toLocaleString();
};

const formatSuccessRate = (value) => {
  if (value == null) return '-';
  const rate = Number(value);
  return Number.isFinite(rate) ? `${rate.toFixed(2)}%` : '-';
};

const getStatusColor = (rate) => {
  if (rate == null) return 'grey';
  if (!Number.isFinite(Number(rate))) return 'grey';
  if (Number(rate) >= 90) return 'green';
  if (Number(rate) >= 70) return 'orange';
  return 'red';
};

const getStatusBarClassName = (rate) => {
  if (rate == null) return 'bg-semi-color-fill-1';
  if (!Number.isFinite(Number(rate))) return 'bg-semi-color-fill-1';
  if (Number(rate) >= 90) return 'bg-green-500';
  if (Number(rate) >= 70) return 'bg-amber-500';
  return 'bg-red-500';
};

const matchesSearch = (model, query) => {
  if (!query) return true;
  const endpointTypes = Array.isArray(model.supported_endpoint_types)
    ? model.supported_endpoint_types.join(' ')
    : '';
  return [
    model.model_name,
    model.vendor_name,
    model.description,
    model.tags,
    endpointTypes,
  ].some((value) =>
    String(value || '')
      .toLowerCase()
      .includes(query),
  );
};

const ModelIdentity = ({ model }) => {
  const iconKey =
    model.icon || model.vendor_icon || model.vendor_name || model.model_name;

  return (
    <div className='flex min-w-0 items-center gap-2.5'>
      <span className='flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-semi-color-fill-0'>
        {getLobeHubIcon(iconKey, 20) || <Boxes size={18} />}
      </span>
      <div className='min-w-0'>
        <div className='truncate font-mono text-sm font-medium text-semi-color-text-0'>
          {model.model_name}
        </div>
        {model.vendor_name ? (
          <div className='truncate text-xs text-semi-color-text-2'>
            {model.vendor_name}
          </div>
        ) : null}
      </div>
    </div>
  );
};

const StatusBadge = ({ perf, t }) => (
  <Tag color={perf ? getStatusColor(perf.success_rate) : 'grey'} shape='circle'>
    {perf ? formatSuccessRate(perf.success_rate) : t('暂无近期数据')}
  </Tag>
);

const RecentSuccessBars = ({ perf, t }) => {
  const recentRates = Array.isArray(perf?.recent_success_rates)
    ? perf.recent_success_rates.filter(
        (rate) => rate != null && Number.isFinite(Number(rate)),
      )
    : [];
  const statusRates = perf
    ? recentRates.length > 0
      ? recentRates.slice(-3)
      : [perf.success_rate]
    : [];
  const paddedRates = [
    ...Array(Math.max(0, 3 - statusRates.length)).fill(null),
    ...statusRates,
  ].slice(-3);

  return (
    <span
      className='flex h-4 items-end gap-1'
      role='img'
      aria-label={
        perf
          ? `${t('状态')}：${formatSuccessRate(perf.success_rate)}`
          : t('暂无近期数据')
      }
    >
      {STATUS_BAR_SLOTS.map((slot, index) => {
        const rate = paddedRates[index];
        const heightClass = index === 0 ? 'h-2' : index === 1 ? 'h-3' : 'h-4';
        return (
          <span
            key={slot}
            title={rate == null ? undefined : formatSuccessRate(rate)}
            className={`w-1.5 rounded-sm ${heightClass} ${getStatusBarClassName(rate)}`}
          />
        );
      })}
    </span>
  );
};

const SummaryItem = ({ icon, label, value }) => (
  <div className='flex min-w-0 items-center gap-3 px-4 py-3'>
    <span className='flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-semi-color-fill-0 text-semi-color-primary'>
      {icon}
    </span>
    <div className='min-w-0'>
      <div className='truncate text-xs text-semi-color-text-2'>{label}</div>
      <div className='text-lg font-semibold tabular-nums text-semi-color-text-0'>
        {value}
      </div>
    </div>
  </div>
);

const MetricValue = ({ label, value }) => (
  <div className='min-w-0'>
    <div className='text-xs text-semi-color-text-2'>{label}</div>
    <div className='mt-0.5 truncate font-mono text-sm tabular-nums text-semi-color-text-0'>
      {value}
    </div>
  </div>
);

const formatTrendHour = (timestamp) => {
  const date = new Date(Number(timestamp) * 1000);
  return `${String(date.getHours()).padStart(2, '0')}:00`;
};

const getTrendRateColor = (rate) => {
  if (!Number.isFinite(Number(rate))) return '#9ca3af';
  if (Number(rate) >= 90) return '#10b981';
  if (Number(rate) >= 70) return '#f59e0b';
  return '#ef4444';
};

const StatusTrendHeader = ({ icon, title, description, accent }) => {
  const Icon = icon;
  return (
    <div className='mb-2 flex flex-wrap items-center justify-between gap-2'>
      <div className='flex min-w-0 items-center gap-2'>
        <Icon size={15} className='text-semi-color-text-2' />
        <div className='min-w-0'>
          <div className='text-sm font-semibold text-semi-color-text-0'>
            {title}
          </div>
          <p className='text-xs text-semi-color-text-2'>{description}</p>
        </div>
      </div>
      {accent ? (
        <div className='text-xs font-medium text-amber-600'>{accent}</div>
      ) : null}
    </div>
  );
};

const StatusTrendPanels = ({ trend, t }) => {
  const latencyValues = trend
    .filter((point) => Number(point.avg_ttft_ms) > 0)
    .map((point) => ({
      time: formatTrendHour(point.ts),
      ttft: Number(point.avg_ttft_ms),
    }));
  const uptimeValues = trend.map((point) => ({
    time: formatTrendHour(point.ts),
    uptime: Math.min(100, Math.max(0, Number(point.success_rate) || 0)),
    incidents: Number(point.success_rate) < 100 ? 1 : 0,
  }));
  const incidentCount = uptimeValues.reduce(
    (count, point) => count + point.incidents,
    0,
  );
  const latencySpec = {
    type: 'line',
    data: [{ id: 'status-latency', values: latencyValues }],
    xField: 'time',
    yField: 'ttft',
    smooth: true,
    line: { style: { stroke: '#6366f1', lineWidth: 2 } },
    point: { visible: true, style: { size: 5 } },
    axes: [
      { orient: 'bottom', tick: { visible: false } },
      { orient: 'left', label: { formatMethod: (value) => `${value} ms` } },
    ],
    tooltip: {
      mark: {
        content: [
          {
            key: t('平均首 Token 延迟'),
            value: (datum) => `${Math.round(datum.ttft)} ms`,
          },
        ],
      },
    },
  };
  const uptimeSpec = {
    type: 'line',
    data: [{ id: 'status-uptime', values: uptimeValues }],
    xField: 'time',
    yField: 'uptime',
    smooth: true,
    line: { style: { stroke: '#10b981', lineWidth: 2 } },
    point: {
      visible: true,
      style: {
        size: 5,
        fill: (datum) => getTrendRateColor(datum.uptime),
      },
    },
    axes: [
      { orient: 'bottom', tick: { visible: false } },
      {
        orient: 'left',
        min: 0,
        max: 100,
        label: { formatMethod: (value) => `${value}%` },
      },
    ],
    tooltip: {
      mark: {
        content: [
          {
            key: t('请求成功率；最近 24 小时 {{incidents}} 个异常桶'),
            value: (datum) => `${Number(datum.uptime).toFixed(2)}%`,
          },
          {
            key: t('异常桶'),
            value: (datum) => `${datum.incidents}`,
          },
        ],
      },
    },
  };

  if (trend.length === 0) return null;

  return (
    <div className='mt-4 grid gap-6 xl:grid-cols-2'>
      <section className='min-w-0 border-t border-semi-color-border pt-4'>
        <StatusTrendHeader
          icon={Timer}
          title={t('延迟趋势（最近 24 小时）')}
          description={t('平均首 Token 延迟')}
        />
        {latencyValues.length > 0 ? (
          <div className='h-56 sm:h-64'>
            <VChart spec={latencySpec} />
          </div>
        ) : (
          <div className='flex h-56 items-center justify-center text-xs text-semi-color-text-2'>
            {t('暂无性能数据')}
          </div>
        )}
      </section>
      <section className='min-w-0 border-t border-semi-color-border pt-4'>
        <StatusTrendHeader
          icon={HeartPulse}
          title={t('可用率（最近 24 小时）')}
          description={
            incidentCount > 0
              ? t('请求成功率；最近 24 小时 {{incidents}} 个异常桶', {
                  incidents: incidentCount,
                })
              : t('请求成功率（最近 24 小时采样）')
          }
          accent={
            incidentCount > 0
              ? t('{{count}} 个异常', { count: incidentCount })
              : null
          }
        />
        <div className='h-56 sm:h-64'>
          <VChart spec={uptimeSpec} />
        </div>
      </section>
    </div>
  );
};

const ModelStatusView = ({
  models = [],
  searchValue = '',
  onSearchChange,
  onCompositionStart,
  onCompositionEnd,
  openModelDetail,
}) => {
  const { t, i18n } = useTranslation();
  const [scope, setScope] = useState(STATUS_SCOPE_RECENT);
  const [metrics, setMetrics] = useState([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState('');
  const [updatedAt, setUpdatedAt] = useState(null);
  const [trend, setTrend] = useState([]);
  const requestIdRef = useRef(0);

  const loadMetrics = useCallback(
    async ({ background = false } = {}) => {
      const requestId = ++requestIdRef.current;
      if (background) setRefreshing(true);
      else setLoading(true);
      setError('');

      try {
        const response = await API.get('/api/perf-metrics/summary', {
          params: { hours: 24 },
          skipErrorHandler: true,
        });
        if (requestId !== requestIdRef.current) return;
        if (!response?.data?.success || !response?.data?.data) {
          throw new Error(response?.data?.message || t('无法加载性能数据'));
        }
        setMetrics(
          Array.isArray(response.data.data.models)
            ? response.data.data.models
            : [],
        );
        setTrend(
          Array.isArray(response.data.data.trend)
            ? response.data.data.trend
            : [],
        );
        setUpdatedAt(new Date());
      } catch (requestError) {
        if (requestId !== requestIdRef.current) return;
        setError(
          requestError?.response?.data?.message ||
            requestError?.message ||
            t('无法加载性能数据'),
        );
        setMetrics([]);
        setTrend([]);
      } finally {
        if (requestId === requestIdRef.current) {
          setLoading(false);
          setRefreshing(false);
        }
      }
    },
    [t],
  );

  useEffect(() => {
    loadMetrics();
    return () => {
      requestIdRef.current += 1;
    };
  }, [loadMetrics]);

  const modelList = Array.isArray(models) ? models : [];
  const normalizedSearch = searchValue.trim().toLowerCase();
  const searchedModels = useMemo(
    () => modelList.filter((model) => matchesSearch(model, normalizedSearch)),
    [modelList, normalizedSearch],
  );
  const catalogModelNames = useMemo(
    () => new Set(modelList.map((model) => model.model_name)),
    [modelList],
  );
  const activeModelNames = useMemo(
    () =>
      new Set(
        metrics
          .filter((perf) => catalogModelNames.has(perf.model_name))
          .map((perf) => perf.model_name),
      ),
    [catalogModelNames, metrics],
  );
  const rows = useMemo(() => {
    const searchedByName = new Map(
      searchedModels.map((model) => [model.model_name, model]),
    );
    const withMetrics = [];
    const included = new Set();

    metrics.forEach((perf) => {
      const model = searchedByName.get(perf.model_name);
      if (!model || included.has(perf.model_name)) return;
      included.add(perf.model_name);
      withMetrics.push({ model, perf });
    });

    if (scope === STATUS_SCOPE_RECENT) return withMetrics;

    const withoutMetrics = searchedModels
      .filter((model) => !included.has(model.model_name))
      .sort((a, b) => a.model_name.localeCompare(b.model_name))
      .map((model) => ({ model }));
    return [...withMetrics, ...withoutMetrics];
  }, [metrics, scope, searchedModels]);

  const updatedAtLabel = useMemo(() => {
    if (!updatedAt) return '';
    return new Intl.DateTimeFormat(
      normalizeLanguage(i18n.resolvedLanguage || i18n.language),
      {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      },
    ).format(updatedAt);
  }, [i18n.language, i18n.resolvedLanguage, updatedAt]);

  const columns = useMemo(
    () => [
      {
        title: t('模型'),
        dataIndex: 'model',
        render: (_, row) => <ModelIdentity model={row.model} />,
      },
      {
        title: t('状态'),
        dataIndex: 'status',
        width: 210,
        render: (_, row) => (
          <div className='flex items-center gap-3'>
            <StatusBadge perf={row.perf} t={t} />
            <RecentSuccessBars perf={row.perf} t={t} />
          </div>
        ),
      },
      {
        title: t('平均延迟'),
        dataIndex: 'latency',
        width: 130,
        align: 'right',
        render: (_, row) => (
          <span className='font-mono text-sm tabular-nums text-semi-color-text-2'>
            {row.perf ? formatLatency(row.perf.avg_latency_ms) : '-'}
          </span>
        ),
      },
      {
        title: t('平均 TTFT'),
        dataIndex: 'ttft',
        width: 130,
        align: 'right',
        render: (_, row) => (
          <span className='font-mono text-sm tabular-nums text-semi-color-text-2'>
            {row.perf ? formatLatency(row.perf.avg_ttft_ms) : '-'}
          </span>
        ),
      },
      {
        title: t('吞吐量'),
        dataIndex: 'throughput',
        width: 130,
        align: 'right',
        render: (_, row) => (
          <span className='font-mono text-sm tabular-nums text-semi-color-text-2'>
            {row.perf ? formatThroughput(row.perf.avg_tps) : '-'}
          </span>
        ),
      },
      {
        title: t('请求次数'),
        dataIndex: 'requests',
        width: 120,
        align: 'right',
        render: (_, row) => (
          <span className='font-mono text-sm tabular-nums text-semi-color-text-2'>
            {row.perf ? formatRequestCount(row.perf.request_count) : '-'}
          </span>
        ),
      },
      {
        title: t('详情'),
        dataIndex: 'actions',
        width: 72,
        align: 'center',
        render: (_, row) => (
          <Button
            type='tertiary'
            theme='borderless'
            icon={<ChevronRight size={16} />}
            aria-label={`${t('详情')}：${row.model.model_name}`}
            onClick={(event) => {
              event.stopPropagation();
              openModelDetail?.(row.model);
            }}
          />
        ),
      },
    ],
    [openModelDetail, t],
  );

  const noDataCount = Math.max(modelList.length - activeModelNames.size, 0);
  const hasSearch = normalizedSearch.length > 0;

  const renderEmptyState = () => (
    <div className='py-10 text-center'>
      <Empty
        title={hasSearch ? t('搜索无结果') : t('暂无性能数据')}
        description={
          hasSearch ? t('模糊搜索模型名称') : t('最近 24 小时的性能指标')
        }
      />
      <Button
        className='mt-3'
        theme='outline'
        type='tertiary'
        onClick={() => {
          if (hasSearch) onSearchChange?.('');
          else setScope(STATUS_SCOPE_ALL);
        }}
      >
        {hasSearch ? t('清除') : t('所有模型')}
      </Button>
    </div>
  );

  return (
    <section className='mx-auto w-full max-w-[1280px] px-2 pb-5 sm:px-4'>
      <div className='flex flex-col gap-4 border-b border-semi-color-border py-4 sm:flex-row sm:items-end sm:justify-between'>
        <div className='min-w-0'>
          <div className='mb-1 flex items-center gap-2 text-semi-color-primary'>
            <Activity size={17} />
            <Text strong type='secondary'>
              {t('模型性能指标')}
            </Text>
          </div>
          <Title heading={4} style={{ margin: 0 }}>
            {t('最近 24 小时的性能指标')}
          </Title>
          <div className='mt-2 flex max-w-2xl items-start gap-1.5 text-xs text-semi-color-text-2'>
            <Info size={14} className='mt-0.5 shrink-0' />
            <span>{t('基于全站真实请求统计，并非主动可用性探测。')}</span>
          </div>
        </div>

        <div className='flex flex-col gap-2 sm:items-end'>
          <Input
            prefix={<Search size={15} />}
            placeholder={t('模糊搜索模型名称')}
            value={searchValue}
            onChange={onSearchChange}
            onCompositionStart={onCompositionStart}
            onCompositionEnd={onCompositionEnd}
            showClear
            className='w-full sm:w-80'
          />
          <div className='flex min-w-0 items-center gap-2'>
            <RadioGroup
              type='button'
              value={scope}
              className='min-w-0 flex-1 sm:flex-none'
              onChange={(event) => setScope(event.target.value)}
            >
              <Radio value={STATUS_SCOPE_RECENT}>
                <span className='flex items-center gap-1.5'>
                  <Activity size={14} />
                  {t('有近期数据的模型')}
                </span>
              </Radio>
              <Radio value={STATUS_SCOPE_ALL}>
                <span className='flex items-center gap-1.5'>
                  <Boxes size={14} />
                  {t('所有模型')}
                </span>
              </Radio>
            </RadioGroup>
            <Button
              type='tertiary'
              theme='outline'
              icon={<RefreshCw size={15} />}
              loading={refreshing}
              aria-label={t('刷新')}
              onClick={() => loadMetrics({ background: true })}
            >
              <span className='hidden lg:inline'>{t('刷新')}</span>
            </Button>
          </div>
        </div>
      </div>

      {error ? (
        <div className='mt-4'>
          <Banner
            type='danger'
            closeIcon={null}
            description={`${t('无法加载性能数据')}：${error}`}
          />
          <div className='mt-3 text-right'>
            <Button theme='outline' type='danger' onClick={() => loadMetrics()}>
              {t('重试')}
            </Button>
          </div>
        </div>
      ) : null}

      <Spin spinning={loading} tip={t('加载中...')}>
        {loading ? (
          <div className='min-h-[320px]' />
        ) : error ? null : (
          <>
            <div className='mt-4 grid grid-cols-1 divide-y divide-semi-color-border border-y border-semi-color-border sm:grid-cols-3 sm:divide-x sm:divide-y-0'>
              <SummaryItem
                icon={<Boxes size={18} />}
                label={t('模型')}
                value={modelList.length}
              />
              <SummaryItem
                icon={<Activity size={18} />}
                label={t('有近期数据的模型')}
                value={activeModelNames.size}
              />
              <SummaryItem
                icon={<CircleHelp size={18} />}
                label={t('暂无近期数据')}
                value={noDataCount}
              />
            </div>

            {updatedAtLabel ? (
              <div className='mt-2 text-right text-xs tabular-nums text-semi-color-text-2'>
                {t('更新时间')}：{updatedAtLabel}
              </div>
            ) : null}

            <StatusTrendPanels trend={trend} t={t} />

            {rows.length === 0 ? (
              renderEmptyState()
            ) : (
              <>
                <div className='mt-4 divide-y divide-semi-color-border overflow-hidden rounded-lg border border-semi-color-border lg:hidden'>
                  {rows.map((row) => (
                    <div
                      key={row.model.id || row.model.model_name}
                      className='p-4'
                    >
                      <div className='flex min-w-0 items-start justify-between gap-3'>
                        <ModelIdentity model={row.model} />
                        <StatusBadge perf={row.perf} t={t} />
                      </div>
                      <div className='mt-3 grid grid-cols-2 gap-4'>
                        <MetricValue
                          label={t('平均延迟')}
                          value={
                            row.perf
                              ? formatLatency(row.perf.avg_latency_ms)
                              : '-'
                          }
                        />
                        <MetricValue
                          label={t('平均 TTFT')}
                          value={
                            row.perf ? formatLatency(row.perf.avg_ttft_ms) : '-'
                          }
                        />
                        <MetricValue
                          label={t('吞吐量')}
                          value={
                            row.perf ? formatThroughput(row.perf.avg_tps) : '-'
                          }
                        />
                        <MetricValue
                          label={t('请求次数')}
                          value={
                            row.perf
                              ? formatRequestCount(row.perf.request_count)
                              : '-'
                          }
                        />
                      </div>
                      <div className='mt-3 flex items-center justify-between gap-3 border-t border-semi-color-border pt-3'>
                        <RecentSuccessBars perf={row.perf} t={t} />
                        <Button
                          type='tertiary'
                          theme='borderless'
                          icon={<ChevronRight size={16} />}
                          iconPosition='right'
                          onClick={() => openModelDetail?.(row.model)}
                        >
                          {t('详情')}
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>

                <div className='mt-4 hidden overflow-hidden rounded-lg border border-semi-color-border lg:block'>
                  <Table
                    columns={columns}
                    dataSource={rows}
                    rowKey={(row) => row.model.id || row.model.model_name}
                    size='small'
                    pagination={
                      rows.length > 20
                        ? { pageSize: 20, showSizeChanger: false }
                        : false
                    }
                    onRow={(row) => ({
                      onClick: () => openModelDetail?.(row.model),
                      style: { cursor: 'pointer' },
                    })}
                  />
                </div>
              </>
            )}
          </>
        )}
      </Spin>
    </section>
  );
};

export default ModelStatusView;
