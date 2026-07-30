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

import React, { useCallback, useEffect, useState } from 'react';
import { Button, Card, Empty, List, Modal, Spin, Tag } from '@douyinfe/semi-ui';
import { IconDesktop, IconExit } from '@douyinfe/semi-icons';
import { API, showError, showSuccess } from '../../../../helpers';

const deviceName = (userAgent, t) => {
  if (!userAgent) return t('未知');

  let browser = '';
  if (userAgent.includes('Edg/')) browser = 'Edge';
  else if (userAgent.includes('Chrome/')) browser = 'Chrome';
  else if (userAgent.includes('Firefox/')) browser = 'Firefox';
  else if (userAgent.includes('Safari/')) browser = 'Safari';

  let system = '';
  if (userAgent.includes('iPhone') || userAgent.includes('iPad')) {
    system = 'iOS';
  } else if (userAgent.includes('Android')) {
    system = 'Android';
  } else if (userAgent.includes('Windows')) {
    system = 'Windows';
  } else if (userAgent.includes('Mac OS')) {
    system = 'macOS';
  } else if (userAgent.includes('Linux')) {
    system = 'Linux';
  }

  return [browser, system].filter(Boolean).join(' - ') || t('未知');
};

const formatSessionTime = (value) => {
  if (!value) return '-';
  return new Date(Number(value) * 1000).toLocaleString();
};

const LoginSessionsCard = ({ t, onCurrentSessionRevoked }) => {
  const [sessions, setSessions] = useState([]);
  const [loading, setLoading] = useState(true);
  const [revokingSessionId, setRevokingSessionId] = useState('');

  const loadSessions = useCallback(async () => {
    setLoading(true);
    try {
      const res = await API.get('/api/user/sessions');
      if (res.data?.success) {
        setSessions(res.data.data || []);
      } else {
        showError(res.data?.message || t('加载失败'));
      }
    } catch (error) {
      showError(
        error.response?.data?.message || error.message || t('加载失败'),
      );
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  const revokeSession = async (session) => {
    setRevokingSessionId(session.sid);
    try {
      const res = await API.delete(
        `/api/user/sessions/${encodeURIComponent(session.sid)}`,
      );
      if (!res.data?.success) {
        showError(res.data?.message || t('操作失败'));
        return;
      }
      if (session.current) {
        onCurrentSessionRevoked();
        return;
      }
      showSuccess(t('注销成功!'));
      await loadSessions();
    } catch (error) {
      showError(
        error.response?.data?.message || error.message || t('操作失败'),
      );
    } finally {
      setRevokingSessionId('');
    }
  };

  const revokeOtherSessions = () => {
    Modal.confirm({
      title: t('注销其他登录会话'),
      content: t('确认'),
      okText: t('注销'),
      cancelText: t('取消'),
      onOk: async () => {
        const res = await API.post('/api/user/sessions/revoke-others');
        if (!res.data?.success) {
          showError(res.data?.message || t('操作失败'));
          return;
        }
        showSuccess(t('注销成功!'));
        await loadSessions();
      },
    });
  };

  const hasOtherSessions = sessions.some((session) => !session.current);

  return (
    <Card className='!rounded-2xl'>
      <div className='mb-4 flex flex-wrap items-center justify-between gap-3'>
        <div>
          <div className='text-lg font-medium'>{t('登录会话')}</div>
          <div className='text-xs text-gray-600'>
            {t('管理当前登录的设备。')}
          </div>
        </div>
        <Button
          type='tertiary'
          theme='outline'
          size='small'
          icon={<IconExit />}
          disabled={!hasOtherSessions || loading}
          onClick={revokeOtherSessions}
        >
          {t('注销其他登录会话')}
        </Button>
      </div>

      {loading ? (
        <div className='flex justify-center py-8'>
          <Spin />
        </div>
      ) : sessions.length === 0 ? (
        <Empty description={t('暂无')} />
      ) : (
        <List
          dataSource={sessions}
          renderItem={(session) => (
            <List.Item
              main={
                <div className='flex min-w-0 items-center gap-3'>
                  <IconDesktop
                    size='large'
                    className='shrink-0 text-gray-500'
                  />
                  <div className='min-w-0'>
                    <div className='flex flex-wrap items-center gap-2 font-medium'>
                      <span>{deviceName(session.user_agent, t)}</span>
                      {session.current && (
                        <Tag color='green'>{t('当前设备')}</Tag>
                      )}
                    </div>
                    <div className='mt-1 text-xs text-gray-500'>
                      {t('IP')}: {session.ip || t('未知')} | {t('登录方式')}:{' '}
                      {session.login_method || t('未知')}
                    </div>
                    <div className='mt-1 text-xs text-gray-500'>
                      {t('最近活跃')}:{' '}
                      {formatSessionTime(session.last_active_at)} |{' '}
                      {t('过期时间')}: {formatSessionTime(session.expires_at)}
                    </div>
                  </div>
                </div>
              }
              extra={
                <Button
                  type='danger'
                  theme='borderless'
                  size='small'
                  loading={revokingSessionId === session.sid}
                  onClick={() => revokeSession(session)}
                >
                  {t('注销')}
                </Button>
              }
            />
          )}
        />
      )}
    </Card>
  );
};

export default LoginSessionsCard;
