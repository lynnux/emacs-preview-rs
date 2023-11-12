# 说明
Emacs org/markdown/html文件预览插件，预览是在浏览器里预览，开启`emacs-preview-mode`后会将当前buffer内容发送给浏览器更新。参考<https://github.com/honmaple/emacs-maple-preview>用rust写的http server，去掉websocket改为js轮询内容是否修改，简单好用。
# 特性
* 支持scroll，不完善将就能用
* 支持相对路径的图片浏览，会自动设置buffer目录到资源加载目录

    
