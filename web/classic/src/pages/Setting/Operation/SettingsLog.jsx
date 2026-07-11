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

import React, { useEffect, useState, useRef } from 'react';
import {
  Button,
  Col,
  Form,
  Row,
  Spin,
  DatePicker,
  Typography,
  Modal,
  Progress,
  Tag,
  Banner,
} from '@douyinfe/semi-ui';
import dayjs from 'dayjs';
import { useTranslation } from 'react-i18next';
import {
  compareObjects,
  API,
  showError,
  showSuccess,
  showWarning,
} from '../../../helpers';

const { Text } = Typography;

export default function SettingsLog(props) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);
  const [loadingCleanHistoryLog, setLoadingCleanHistoryLog] = useState(false);
  const [cleanupTask, setCleanupTask] = useState(null);
  const cleanupNotificationRef = useRef('');
  const [inputs, setInputs] = useState({
    LogConsumeEnabled: false,
    historyTimestamp: dayjs().subtract(1, 'month').toDate(),
  });
  const refForm = useRef();
  const [inputsRow, setInputsRow] = useState(inputs);
  const cleanupActive =
    cleanupTask?.status === 'pending' || cleanupTask?.status === 'running';
  const cleanupProgress = Math.min(
    100,
    Math.max(0, Number(cleanupTask?.state?.progress) || 0),
  );
  const cleanupProcessed = Number(cleanupTask?.state?.processed) || 0;
  const cleanupTotal = Number(cleanupTask?.state?.total) || 0;

  useEffect(() => {
    let active = true;
    API.get('/api/system-task/current', {
      params: { type: 'log_cleanup' },
      skipErrorHandler: true,
    })
      .then((res) => {
        if (active && res.data?.success && res.data?.data) {
          setCleanupTask(res.data.data);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!cleanupTask?.task_id || !cleanupActive) return undefined;
    let active = true;
    const interval = window.setInterval(async () => {
      try {
        const res = await API.get(`/api/system-task/${cleanupTask.task_id}`, {
          skipErrorHandler: true,
        });
        if (!active || !res.data?.success || !res.data?.data) return;
        const nextTask = res.data.data;
        setCleanupTask(nextTask);
        if (
          nextTask.status === 'succeeded' &&
          cleanupNotificationRef.current !== nextTask.task_id
        ) {
          cleanupNotificationRef.current = nextTask.task_id;
          const count =
            Number(nextTask.result?.deleted_count) ||
            Number(nextTask.state?.processed) ||
            0;
          showSuccess(
            count > 0
              ? t('{{count}} 条日志已清理！', { count })
              : t('没有匹配所选时间的日志'),
          );
        } else if (
          nextTask.status === 'failed' &&
          cleanupNotificationRef.current !== nextTask.task_id
        ) {
          cleanupNotificationRef.current = nextTask.task_id;
          showError(nextTask.error || t('日志清理失败'));
        }
      } catch (error) {
        // Keep polling while the backend task remains active.
      }
    }, 1000);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [cleanupActive, cleanupTask?.task_id, t]);

  function onSubmit() {
    const updateArray = compareObjects(inputs, inputsRow).filter(
      (item) => item.key !== 'historyTimestamp',
    );

    if (!updateArray.length) return showWarning(t('你似乎并没有修改什么'));
    const requestQueue = updateArray.map((item) => {
      let value = '';
      if (typeof inputs[item.key] === 'boolean') {
        value = String(inputs[item.key]);
      } else {
        value = inputs[item.key];
      }
      return API.put('/api/option/', {
        key: item.key,
        value,
      });
    });
    setLoading(true);
    Promise.all(requestQueue)
      .then((res) => {
        if (requestQueue.length === 1) {
          if (res.includes(undefined)) return;
        } else if (requestQueue.length > 1) {
          if (res.includes(undefined))
            return showError(t('部分保存失败，请重试'));
        }
        showSuccess(t('保存成功'));
        props.refresh();
      })
      .catch(() => {
        showError(t('保存失败，请重试'));
      })
      .finally(() => {
        setLoading(false);
      });
  }
  async function onCleanHistoryLog() {
    if (!inputs.historyTimestamp) {
      showError(t('请选择日志记录时间'));
      return;
    }

    const now = dayjs();
    const targetDate = dayjs(inputs.historyTimestamp);
    const targetTime = targetDate.format('YYYY-MM-DD HH:mm:ss');
    const currentTime = now.format('YYYY-MM-DD HH:mm:ss');
    const daysDiff = now.diff(targetDate, 'day');

    Modal.confirm({
      title: t('确认清除历史日志'),
      content: (
        <div style={{ lineHeight: '1.8' }}>
          <p>
            <Text>{t('当前时间')}：</Text>
            <Text strong style={{ color: '#52c41a' }}>
              {currentTime}
            </Text>
          </p>
          <p>
            <Text>{t('选择时间')}：</Text>
            <Text strong type='danger'>
              {targetTime}
            </Text>
            {daysDiff > 0 && (
              <Text type='tertiary'>
                {' '}
                ({t('约')} {daysDiff} {t('天前')})
              </Text>
            )}
          </p>
          <div
            style={{
              background: '#fff7e6',
              border: '1px solid #ffd591',
              padding: '12px',
              borderRadius: '4px',
              marginTop: '12px',
              color: '#333',
            }}
          >
            <Text strong style={{ color: '#d46b08' }}>
              ⚠️ {t('注意')}：
            </Text>
            <Text style={{ color: '#333' }}>{t('将删除')} </Text>
            <Text strong style={{ color: '#cf1322' }}>
              {targetTime}
            </Text>
            {daysDiff > 0 && (
              <Text style={{ color: '#8c8c8c' }}>
                {' '}
                ({t('约')} {daysDiff} {t('天前')})
              </Text>
            )}
            <Text style={{ color: '#333' }}> {t('之前的所有日志')}</Text>
          </div>
          <p style={{ marginTop: '12px' }}>
            <Text type='danger'>
              {t('此操作不可恢复，请仔细确认时间后再操作！')}
            </Text>
          </p>
        </div>
      ),
      okText: t('确认删除'),
      cancelText: t('取消'),
      okType: 'danger',
      onOk: async () => {
        try {
          setLoadingCleanHistoryLog(true);
          const targetTimestamp = Math.floor(
            new Date(inputs.historyTimestamp).getTime() / 1000,
          );
          const res = await API.post('/api/system-task/log-cleanup', null, {
            params: { target_timestamp: targetTimestamp },
            skipErrorHandler: true,
          });
          const { success, message, data } = res.data;
          if (!success || !data) {
            throw new Error(message || t('日志清理失败'));
          }
          cleanupNotificationRef.current = '';
          setCleanupTask(data);
          showSuccess(t('日志清理任务已启动'));
        } catch (error) {
          showError(
            error?.response?.data?.message ||
              error?.message ||
              t('日志清理失败'),
          );
        } finally {
          setLoadingCleanHistoryLog(false);
        }
      },
    });
  }

  useEffect(() => {
    const currentInputs = {};
    for (let key in props.options) {
      if (Object.keys(inputs).includes(key)) {
        currentInputs[key] = props.options[key];
      }
    }
    currentInputs['historyTimestamp'] = inputs.historyTimestamp;
    setInputs(Object.assign(inputs, currentInputs));
    setInputsRow(structuredClone(currentInputs));
    refForm.current.setValues(currentInputs);
  }, [props.options]);
  return (
    <>
      <Spin spinning={loading}>
        <Form
          values={inputs}
          getFormApi={(formAPI) => (refForm.current = formAPI)}
          style={{ marginBottom: 15 }}
        >
          <Form.Section text={t('日志设置')}>
            <Row gutter={16}>
              <Col xs={24} sm={12} md={8} lg={8} xl={8}>
                <Form.Switch
                  field={'LogConsumeEnabled'}
                  label={t('启用额度消费日志记录')}
                  size='default'
                  checkedText='｜'
                  uncheckedText='〇'
                  onChange={(value) => {
                    setInputs({
                      ...inputs,
                      LogConsumeEnabled: value,
                    });
                  }}
                />
              </Col>
              <Col xs={24} sm={12} md={8} lg={8} xl={8}>
                <Spin spinning={loadingCleanHistoryLog}>
                  <Form.DatePicker
                    label={t('清除历史日志')}
                    field={'historyTimestamp'}
                    type='dateTime'
                    inputReadOnly={true}
                    onChange={(value) => {
                      setInputs({
                        ...inputs,
                        historyTimestamp: value,
                      });
                    }}
                  />
                  <Text
                    type='tertiary'
                    size='small'
                    style={{ display: 'block', marginTop: 4, marginBottom: 8 }}
                  >
                    {t('将清除选定时间之前的所有日志')}
                  </Text>
                  <Button
                    size='default'
                    type='danger'
                    onClick={onCleanHistoryLog}
                    disabled={cleanupActive}
                  >
                    {cleanupActive ? t('清理中...') : t('清除历史日志')}
                  </Button>
                  {cleanupTask && (
                    <div className='mt-3 rounded-md border border-semi-color-border p-3'>
                      <div className='mb-2 flex items-center justify-between gap-3'>
                        <Text strong>{t('日志清理进度')}</Text>
                        <Tag
                          color={
                            cleanupTask.status === 'succeeded'
                              ? 'green'
                              : cleanupTask.status === 'failed'
                                ? 'red'
                                : 'blue'
                          }
                        >
                          {t(cleanupTask.status)}
                        </Tag>
                      </div>
                      <Progress
                        percent={cleanupProgress}
                        showInfo={false}
                        size='small'
                      />
                      <Text type='tertiary' size='small' className='mt-2 block'>
                        {t('已处理 {{processed}} / {{total}} 条日志', {
                          processed: cleanupProcessed,
                          total: cleanupTotal,
                        })}
                      </Text>
                      {cleanupTask.status === 'failed' && cleanupTask.error ? (
                        <Banner
                          type='danger'
                          closeIcon={null}
                          description={cleanupTask.error}
                          className='mt-2'
                        />
                      ) : null}
                    </div>
                  )}
                </Spin>
              </Col>
            </Row>

            <Row>
              <Button size='default' onClick={onSubmit}>
                {t('保存日志设置')}
              </Button>
            </Row>
          </Form.Section>
        </Form>
      </Spin>
    </>
  );
}
