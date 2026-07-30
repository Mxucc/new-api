package controller

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/QuantumNous/new-api/common"
	"github.com/QuantumNous/new-api/model"
	"github.com/QuantumNous/new-api/setting/system_setting"
	"github.com/gin-gonic/gin"
	"github.com/glebarez/sqlite"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"gorm.io/gorm"
)

func TestUpdateOptionRejectsUnsupportedFrontendTheme(t *testing.T) {
	response := httptest.NewRecorder()
	context, _ := gin.CreateTestContext(response)
	context.Request = httptest.NewRequest(
		http.MethodPut,
		"/api/option/",
		strings.NewReader(`{"key":"theme.frontend","value":"unsupported"}`),
	)

	UpdateOption(context)

	assert.Equal(t, http.StatusOK, response.Code)
	assert.JSONEq(t, `{"success":false,"message":"无效的主题值，可选值：default（新版前端）、classic（经典前端）"}`, response.Body.String())
}

func TestUpdateOptionAcceptsClassicFrontendTheme(t *testing.T) {
	previousDB := model.DB
	previousMap := common.OptionMap
	previousTheme := common.GetTheme()
	previousThemeSetting := system_setting.GetThemeSettings().Frontend
	db, err := gorm.Open(sqlite.Open(":memory:"), &gorm.Config{})
	require.NoError(t, err)
	require.NoError(t, db.AutoMigrate(&model.Option{}))
	model.DB = db
	common.OptionMap = map[string]string{}
	t.Cleanup(func() {
		model.DB = previousDB
		common.OptionMap = previousMap
		system_setting.GetThemeSettings().Frontend = previousThemeSetting
		common.SetTheme(previousTheme)
	})

	response := httptest.NewRecorder()
	context, _ := gin.CreateTestContext(response)
	context.Request = httptest.NewRequest(
		http.MethodPut,
		"/api/option/",
		strings.NewReader(`{"key":"theme.frontend","value":"classic"}`),
	)

	UpdateOption(context)

	assert.Equal(t, http.StatusOK, response.Code)
	assert.JSONEq(t, `{"success":true,"message":""}`, response.Body.String())
	assert.Equal(t, "classic", common.GetTheme())
	var option model.Option
	require.NoError(t, db.Where(&model.Option{Key: "theme.frontend"}).First(&option).Error)
	assert.Equal(t, "classic", option.Value)
}

func TestGetStatusAdvertisesSelectedFrontendTheme(t *testing.T) {
	previousTheme := common.GetTheme()
	common.SetTheme("classic")
	t.Cleanup(func() { common.SetTheme(previousTheme) })
	response := httptest.NewRecorder()
	context, _ := gin.CreateTestContext(response)
	context.Request = httptest.NewRequest(http.MethodGet, "/api/status", nil)

	GetStatus(context)

	var payload struct {
		Success bool           `json:"success"`
		Data    map[string]any `json:"data"`
	}
	require.NoError(t, common.Unmarshal(response.Body.Bytes(), &payload))
	assert.True(t, payload.Success)
	assert.Equal(t, "classic", payload.Data["theme"])
}
