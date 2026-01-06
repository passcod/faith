set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/variance.png'
set title 'Performance Variance (Google, 10 requests)'
set xlabel ''
set ylabel 'Duration (ms)'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("1" 0, "10" 1, "100" 2)
set offsets 0.5, 0.5, 0, 0
set style fill solid 0.5
set boxwidth 0.5
set xtics rotate by -45
set key off

plot 'charts/variance_data.txt' using 0:2:1:5:4:xtic(6) with candlesticks whiskerbars lw 2 title 'Min/Max', \
     '' using 0:3:3:3:3 with candlesticks lw 2 lt -1 notitle
