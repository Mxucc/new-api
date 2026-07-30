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
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Banner,
  Button,
  Card,
  Empty,
  Radio,
  RadioGroup,
  Space,
  Spin,
  Table,
  Tag,
  Typography,
} from '@douyinfe/semi-ui';
import {
  BarChart3,
  Building2,
  RefreshCw,
  TrendingDown,
  TrendingUp,
  Trophy,
} from 'lucide-react';
import { API } from '../../helpers';
import { getLobeHubIcon } from '../../helpers/render';

const { Text, Title } = Typography;
const VALID_PERIODS = new Set(['today', 'week', 'month', 'year']);

const formatTokens = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return '0';
  if (numeric >= 1000000000000) {
    return `${(numeric / 1000000000000).toFixed(2)}T`;
  }
  if (numeric >= 1000000000) {
    return `${(numeric / 1000000000).toFixed(numeric >= 10000000000 ? 1 : 2)}B`;
  }
  if (numeric >= 1000000) {
    return `${(numeric / 1000000).toFixed(numeric >= 10000000 ? 1 : 2)}M`;
  }
  if (numeric >= 1000) {
    return `${(numeric / 1000).toFixed(numeric >= 10000 ? 0 : 1)}K`;
  }
  return numeric.toLocaleString();
};

const formatShare = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return '0%';
  if (numeric < 0.001) return '<0.1%';
  return `${(numeric * 100).toFixed(numeric < 0.01 ? 2 : 1)}%`;
};

const GrowthTag = ({ value }) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric === 0) {
    return <Tag color='grey'>0%</Tag>;
  }
  return (
    <Tag color={numeric > 0 ? 'green' : 'red'}>
      {numeric > 0 ? '+' : ''}
      {numeric.toFixed(1)}%
    </Tag>
  );
};

const RankMovement = ({ rank, previousRank, t }) => {
  const previous = Number(previousRank);
  if (!Number.isFinite(previous) || previous <= 0) {
    return <Tag color='blue'>{t('新上榜')}</Tag>;
  }

  const delta = previous - Number(rank);
  if (delta === 0) return <Text type='tertiary'>-</Text>;

  return (
    <span
      className={`inline-flex items-center gap-1 text-xs font-medium ${
        delta > 0 ? 'text-green-600' : 'text-red-500'
      }`}
    >
      {delta > 0 ? <TrendingUp size={13} /> : <TrendingDown size={13} />}
      {Math.abs(delta)}
    </span>
  );
};

const RankingIdentity = ({ name, vendor, icon }) => (
  <div className='flex min-w-0 items-center gap-2'>
    <span className='flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-semi-color-fill-0'>
      {getLobeHubIcon(icon || vendor || name, 20)}
    </span>
    <div className='min-w-0'>
      <div className='truncate text-sm font-medium text-semi-color-text-0'>
        {name || '-'}
      </div>
      {vendor ? (
        <div className='truncate text-xs text-semi-color-text-2'>{vendor}</div>
      ) : null}
    </div>
  </div>
);

const SummaryItem = ({ icon, label, value }) => (
  <div className='flex min-w-0 items-center gap-3 px-4 py-3'>
    <span className='flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-semi-color-fill-0 text-semi-color-primary'>
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

const MoversList = ({ title, rows, direction, t }) => (
  <section className='min-w-0'>
    <div className='mb-3 flex items-center gap-2'>
      {direction === 'up' ? (
        <TrendingUp size={16} className='text-green-600' />
      ) : (
        <TrendingDown size={16} className='text-red-500' />
      )}
      <Text strong>{title}</Text>
    </div>
    {rows.length === 0 ? (
      <div className='rounded-md border border-dashed border-semi-color-border px-3 py-7 text-center text-sm text-semi-color-text-2'>
        {t('暂无排行数据')}
      </div>
    ) : (
      <div className='divide-y divide-semi-color-border'>
        {rows.slice(0, 6).map((row, index) => (
          <div
            key={`${row.model_name}-${index}`}
            className='flex items-center justify-between gap-3 py-2.5'
          >
            <RankingIdentity
              name={row.model_name}
              vendor={row.vendor}
              icon={row.vendor_icon}
            />
            <div className='shrink-0 text-right'>
              <div
                className={`text-sm font-semibold ${
                  direction === 'up' ? 'text-green-600' : 'text-red-500'
                }`}
              >
                {direction === 'up' ? '+' : '-'}
                {Math.abs(Number(row.rank_delta) || 0)}
              </div>
              <div className='text-xs text-semi-color-text-2'>
                #{row.current_rank || '-'}
              </div>
            </div>
          </div>
        ))}
      </div>
    )}
  </section>
);

const Rankings = () => {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedPeriod = searchParams.get('period');
  const period = VALID_PERIODS.has(requestedPeriod) ? requestedPeriod : 'week';
  const [snapshot, setSnapshot] = useState(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState('');
  const [updatedAt, setUpdatedAt] = useState(null);
  const requestIdRef = useRef(0);

  const loadRankings = useCallback(
    async ({ background = false } = {}) => {
      const requestId = ++requestIdRef.current;
      if (background) setRefreshing(true);
      else setLoading(true);
      setError('');

      try {
        const res = await API.get('/api/rankings', {
          params: { period },
          skipErrorHandler: true,
        });
        if (requestId !== requestIdRef.current) return;
        if (!res?.data?.success || !res?.data?.data) {
          throw new Error(res?.data?.message || t('加载排行榜失败'));
        }
        setSnapshot(res.data.data);
        setUpdatedAt(new Date());
      } catch (requestError) {
        if (requestId !== requestIdRef.current) return;
        setError(
          requestError?.response?.data?.message ||
            requestError?.message ||
            t('加载排行榜失败'),
        );
        setSnapshot(null);
      } finally {
        if (requestId === requestIdRef.current) {
          setLoading(false);
          setRefreshing(false);
        }
      }
    },
    [period, t],
  );

  useEffect(() => {
    loadRankings();
    return () => {
      requestIdRef.current += 1;
    };
  }, [loadRankings]);

  const models = Array.isArray(snapshot?.models) ? snapshot.models : [];
  const vendors = Array.isArray(snapshot?.vendors) ? snapshot.vendors : [];
  const movers = Array.isArray(snapshot?.top_movers) ? snapshot.top_movers : [];
  const droppers = Array.isArray(snapshot?.top_droppers)
    ? snapshot.top_droppers
    : [];

  const totalTokens = useMemo(
    () => models.reduce((sum, row) => sum + (Number(row.total_tokens) || 0), 0),
    [models],
  );

  const modelColumns = useMemo(
    () => [
      {
        title: t('排名'),
        dataIndex: 'rank',
        width: 72,
        render: (value) => (
          <span className='font-semibold tabular-nums'>#{value}</span>
        ),
      },
      {
        title: t('模型'),
        dataIndex: 'model_name',
        width: 260,
        render: (_, row) => (
          <RankingIdentity
            name={row.model_name}
            vendor={row.vendor}
            icon={row.vendor_icon}
          />
        ),
      },
      {
        title: t('令牌数'),
        dataIndex: 'total_tokens',
        width: 130,
        render: (value) => (
          <span className='font-medium tabular-nums'>
            {formatTokens(value)}
          </span>
        ),
      },
      {
        title: t('市场份额'),
        dataIndex: 'share',
        width: 120,
        render: (value) => formatShare(value),
      },
      {
        title: t('增长率'),
        dataIndex: 'growth_pct',
        width: 110,
        render: (value) => <GrowthTag value={value} />,
      },
      {
        title: t('排名变化'),
        dataIndex: 'previous_rank',
        width: 110,
        render: (value, row) => (
          <RankMovement rank={row.rank} previousRank={value} t={t} />
        ),
      },
    ],
    [t],
  );

  const vendorColumns = useMemo(
    () => [
      {
        title: t('排名'),
        dataIndex: 'rank',
        width: 72,
        render: (value) => (
          <span className='font-semibold tabular-nums'>#{value}</span>
        ),
      },
      {
        title: t('供应商'),
        dataIndex: 'vendor',
        width: 230,
        render: (_, row) => (
          <RankingIdentity name={row.vendor} icon={row.vendor_icon} />
        ),
      },
      {
        title: t('令牌数'),
        dataIndex: 'total_tokens',
        width: 120,
        render: (value) => formatTokens(value),
      },
      {
        title: t('市场份额'),
        dataIndex: 'share',
        width: 110,
        render: (value) => formatShare(value),
      },
      {
        title: t('模型数量'),
        dataIndex: 'models_count',
        width: 110,
      },
      {
        title: t('热门模型'),
        dataIndex: 'top_model',
        width: 190,
        render: (value) => value || '-',
      },
      {
        title: t('增长率'),
        dataIndex: 'growth_pct',
        width: 110,
        render: (value) => <GrowthTag value={value} />,
      },
    ],
    [t],
  );

  const periodOptions = [
    { value: 'today', label: t('今日') },
    { value: 'week', label: t('本周') },
    { value: 'month', label: t('本月') },
    { value: 'year', label: t('本年') },
  ];

  const emptyContent = (
    <Empty
      title={t('暂无排行数据')}
      description={t('当前时间范围内还没有可展示的用量数据')}
    />
  );

  return (
    <main className='w-full bg-semi-color-bg-0 pt-16'>
      <div className='mx-auto w-full max-w-[1280px] px-3 py-6 sm:px-6 sm:py-8'>
        <div className='flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between'>
          <div className='min-w-0'>
            <div className='mb-2 flex items-center gap-2 text-semi-color-primary'>
              <Trophy size={18} />
              <Text strong type='secondary'>
                {t('实时用量排行')}
              </Text>
            </div>
            <Title heading={2} style={{ margin: 0 }}>
              {t('排行榜')}
            </Title>
            <Text type='tertiary' className='mt-2 block max-w-2xl'>
              {t('查看平台中使用量最高的模型与供应商，以及排名变化趋势。')}
            </Text>
          </div>
          <div className='flex flex-wrap items-center gap-2'>
            <RadioGroup
              type='button'
              value={period}
              onChange={(event) => {
                setSearchParams({ period: event.target.value });
              }}
            >
              {periodOptions.map((option) => (
                <Radio key={option.value} value={option.value}>
                  {option.label}
                </Radio>
              ))}
            </RadioGroup>
            <Button
              type='tertiary'
              theme='outline'
              icon={<RefreshCw size={15} />}
              loading={refreshing}
              onClick={() => loadRankings({ background: true })}
            >
              {t('刷新')}
            </Button>
          </div>
        </div>

        <div className='mt-6 grid grid-cols-1 divide-y divide-semi-color-border border-y border-semi-color-border sm:grid-cols-3 sm:divide-x sm:divide-y-0'>
          <SummaryItem
            icon={<BarChart3 size={18} />}
            label={t('总令牌数')}
            value={formatTokens(totalTokens)}
          />
          <SummaryItem
            icon={<Trophy size={18} />}
            label={t('领先模型')}
            value={models[0]?.model_name || '-'}
          />
          <SummaryItem
            icon={<Building2 size={18} />}
            label={t('领先供应商')}
            value={vendors[0]?.vendor || '-'}
          />
        </div>

        {updatedAt ? (
          <div className='mt-2 text-right text-xs text-semi-color-text-2'>
            {t('更新时间')}：{updatedAt.toLocaleString()}
          </div>
        ) : null}

        {error ? (
          <Banner
            type='danger'
            closeIcon={null}
            description={error}
            className='mt-5'
          />
        ) : null}

        <Spin spinning={loading} tip={t('加载中...')}>
          <div className='mt-5 grid grid-cols-1 gap-5'>
            <Card
              title={
                <Space>
                  <Trophy size={16} />
                  <span>{t('模型排行')}</span>
                </Space>
              }
              bodyStyle={{ padding: 0 }}
            >
              {models.length === 0 && !loading ? (
                <div className='py-10'>{emptyContent}</div>
              ) : (
                <Table
                  columns={modelColumns}
                  dataSource={models}
                  rowKey={(row) => `${row.rank}-${row.model_name}`}
                  pagination={false}
                  size='small'
                  scroll={{ x: 800 }}
                />
              )}
            </Card>

            <Card
              title={
                <Space>
                  <Building2 size={16} />
                  <span>{t('供应商排行')}</span>
                </Space>
              }
              bodyStyle={{ padding: 0 }}
            >
              {vendors.length === 0 && !loading ? (
                <div className='py-10'>{emptyContent}</div>
              ) : (
                <Table
                  columns={vendorColumns}
                  dataSource={vendors}
                  rowKey={(row) => `${row.rank}-${row.vendor}`}
                  pagination={false}
                  size='small'
                  scroll={{ x: 900 }}
                />
              )}
            </Card>

            <Card title={t('排名动向')}>
              <div className='grid grid-cols-1 gap-6 md:grid-cols-2'>
                <MoversList
                  title={t('上升最快')}
                  rows={movers}
                  direction='up'
                  t={t}
                />
                <MoversList
                  title={t('下降最快')}
                  rows={droppers}
                  direction='down'
                  t={t}
                />
              </div>
            </Card>
          </div>
        </Spin>
      </div>
    </main>
  );
};

export default Rankings;
